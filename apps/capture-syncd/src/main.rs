//! The sync daemon.
//!
//! # Why this is its own process
//!
//! The resident overlay is what the capture gesture waits on, and §8.10
//! freezes that gesture's latency for this milestone. A blocking accept, a
//! peer that stalls halfway through a transfer, an mDNS responder thread —
//! none of it can delay the chord if none of it runs in that process.
//!
//! # Why it is not a second writer
//!
//! §8.8 elects one writer. The overlay does not hold that election
//! continuously: it takes the writer lock per operation and treats contention
//! as ordinary, which is what its due-work tick already does. So this daemon
//! does the same — it reads without the lock, and takes it briefly to hand
//! arriving events to the same [`Gate`] a local capture goes through. Failing
//! to take it means somebody else is writing, which is not an error and is not
//! reported as one.

mod nudge;

use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

use capture_core::roster::Roster;
use capture_store::gate::Gate;
use capture_store::identity::Identity;
use capture_store::ops::Ops;
use capture_store::paths::StorePaths;
use capture_store::session::Session;
use capture_store::sync::StoreArchive;
use capture_sync::anti_entropy::{Exchanged, exchange};
use capture_sync::discovery::{self, Network};
use capture_sync::mdns::Beacon;
use capture_sync::session::Session as Secure;
use capture_sync::tcp::Tcp;

/// How often to look around when nothing is happening.
const IDLE: Duration = Duration::from_secs(30);

/// How often to check whether this device has written anything.
///
/// A capture should reach the other device promptly, and thirty seconds is not
/// prompt. §5.4 describes this as a live push to open sessions; what is here
/// instead is a short poll of the local log that runs a round as soon as
/// something is written.
///
/// Polling rather than being told, for the reason `capture_store::watch` gives:
/// a reader that depends on a notification shows stale data for ever if the
/// message is dropped or the writer died between committing and sending it,
/// where a reader that looks for itself is at worst late. The honest
/// difference from §5.4 is latency — one round rather than one socket write —
/// and it is written down rather than claimed away.
const LOOK: Duration = Duration::from_secs(2);

/// What to say when asked, rather than starting.
const USAGE: &str = "\
capture-syncd — find paired devices and reconcile with them

    capture-syncd            run in the foreground until stopped
    capture-syncd --help     this

It takes no other options. Where the store lives comes from $CAPTURE_HOME or the
usual place, and the port from $CAPTURE_SYNC_PORT — which exists for the one case
of two stores on one machine, and is otherwise best left alone.

It reads the store without the writer lock and takes it only for as long as it
takes to hand arriving events over, so it can run alongside the overlay and the
CLI without contending with either.

Sync does nothing until a second device exists. To add one:  capture pair
";

fn main() {
    // Anything but a bare invocation prints this. A daemon that answered an
    // unrecognised flag by opening a listening socket would be one that
    // started when somebody was trying to find out what it does.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        let asked = args.iter().any(|a| a == "--help" || a == "-h");
        print!("{USAGE}");
        std::process::exit(if asked { 0 } else { 2 });
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "capture_syncd=info,capture_sync=info".into()),
        )
        .init();

    if let Err(e) = run() {
        tracing::error!(%e, "the sync daemon stopped");
        std::process::exit(1);
    }
}

fn run() -> Result<(), capture_store::Error> {
    let paths = StorePaths::resolve().ok_or_else(|| {
        capture_store::Error::Usage(
            "cannot work out where the store should live: no home directory, and \
             $CAPTURE_HOME is not set"
                .into(),
        )
    })?;
    let identity = capture_store::store::open(&paths)?;

    tracing::info!(origin = ?identity.dev().short(), "sync daemon starting");

    // Incoming connections get their own thread each. One thread per peer is
    // the shape the transport was written for — `Transport` requires `Send`
    // precisely because of this — and with a handful of devices the count is a
    // handful.
    let port = discovery::sync_port();
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    let (tx, rx) = mpsc::channel();
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = tx.send(Arrival::Dialled(stream));
            }
        });
    }

    // Advertising is best-effort. mDNS is blocked on many corporate and guest
    // networks, and a daemon that refused to run without it would refuse
    // exactly where the static fallback exists to help.
    // The two moments a timer handles worst: a laptop that has just been
    // opened, and a machine that has just joined the network its peers are on.
    {
        let tx = tx.clone();
        nudge::watch_into(tx);
    }

    let mut beacon = match Beacon::new() {
        Ok(b) => Some(b),
        Err(e) => {
            tracing::info!(%e, "no mDNS on this network; falling back to remembered addresses");
            None
        }
    };

    // What this device has written, so a capture does not wait for the idle
    // timer.
    let mut watch = {
        let log = capture_store::log::Log::open_read(&paths.events)?;
        let ops = Ops::open_read(&paths.ops)?;
        capture_store::watch::Watch::start(&log, &ops)?
    };

    let mut round = 0u64;
    loop {
        if let Err(e) = tick(&paths, &identity, &mut beacon, round, port) {
            // A round that fails is a round. The next one may find a different
            // network, a woken peer, or a released lock.
            tracing::warn!(%e, "sync round did not complete");
        }
        round += 1;

        // Answer whoever calls, and go round again as soon as this device
        // writes something — or when the idle timer comes up, whichever is
        // first.
        let deadline = IDLE;
        let mut waited = Duration::ZERO;
        loop {
            match rx.recv_timeout(LOOK) {
                Ok(Arrival::Dialled(stream)) => {
                    if let Err(e) = answer(&paths, &identity, stream) {
                        tracing::info!(%e, "a peer connection ended");
                    }
                }
                Ok(Arrival::Nudged(what)) => {
                    if what == nudge::Nudge::Network {
                        // Everything discovery holds belongs to the old
                        // network: remembered addresses point at it, and the
                        // pseudonyms were derived against it, so peers here
                        // would not be recognised even if they were visible.
                        if let Some(b) = beacon.as_mut() {
                            b.forget();
                        }
                    }
                    tracing::debug!(?what, "syncing now rather than on the timer");
                    break;
                }
                Err(_) => {
                    if wrote_something(&paths, &mut watch) {
                        tracing::debug!("something was captured here; syncing now");
                        break;
                    }
                    waited += LOOK;
                    if waited >= deadline {
                        break;
                    }
                }
            }
        }
    }
}

/// Whether this device has written anything since the last look.
///
/// Errors are treated as "nothing", not as failures: the store may be
/// momentarily locked by the overlay, and a daemon that stopped over that
/// would stop whenever the person was capturing.
fn wrote_something(paths: &StorePaths, watch: &mut capture_store::watch::Watch) -> bool {
    let Ok(log) = capture_store::log::Log::open_read(&paths.events) else {
        return false;
    };
    let Ok(ops) = Ops::open_read(&paths.ops) else {
        return false;
    };
    watch.poll(&log, &ops).map(|c| c.any()).unwrap_or(false)
}

enum Arrival {
    Dialled(std::net::TcpStream),
    /// The machine woke, or the network changed under it.
    Nudged(nudge::Nudge),
}

/// One round: advertise what this device holds, and reconcile with whoever is
/// reachable.
fn tick(
    paths: &StorePaths,
    identity: &Identity,
    beacon: &mut Option<Beacon>,
    round: u64,
    port: u16,
) -> Result<(), capture_store::Error> {
    let (roster, heads) = look(paths)?;
    let Some(secret) = roster.secret else {
        // An archive written before sync existed. It has never advertised and
        // has nothing to advertise under; minting a key is an event some
        // device has to author, not something to invent on read.
        tracing::debug!("no roster secret yet; not advertising");
        return Ok(());
    };

    let net = Network::of(&network_evidence());
    let epoch = discovery::epoch_at(now() / 1000);
    let me = identity.public().to_bytes();

    let heads_pairs: Vec<(capture_core::Dev, u64)> = heads
        .iter()
        .filter_map(|(dev, seq)| parse_dev(dev).map(|d| (d, *seq)))
        .collect();

    if let Some(b) = beacon {
        let pseudonym = discovery::pseudonym(&secret, &net, epoch, &me);
        let digest = discovery::digest(&secret, &net, epoch, &heads_pairs);
        if let Err(e) = b.advertise(&pseudonym, &digest, port, &local_addresses()) {
            tracing::info!(%e, "could not advertise");
        }
    }

    let ops = Ops::open(&paths.ops)?;

    // The roster, plus anyone pairing taught this device about. A device that
    // has just joined is not yet in its own roster — the admission lives in
    // the *other* device's log — so a daemon that only ever dialled roster
    // members would never make the first connection and would never receive
    // the log that would put it in the roster. That deadlock is why the
    // pairing record is consulted here rather than only in the surfaces.
    let mut members: Vec<[u8; 32]> = roster.keys.iter().copied().filter(|k| k != &me).collect();
    for k in ops.dialable()? {
        if k != me && !members.contains(&k) {
            members.push(k);
        }
    }
    if members.is_empty() {
        return Ok(());
    }
    let mut reachable = Vec::new();

    // Whoever is advertising, plus whoever was remembered. The two produce the
    // same kind of answer on purpose: a sighting is a sighting whether it came
    // from a broadcast or from a note of where a peer used to be.
    if let Some(b) = beacon
        && let Ok(sightings) = b.sightings()
    {
        for s in sightings {
            if let Some(seen) = discovery::consider(&secret, &net, epoch, &members, &s) {
                reachable.push((seen.peer, seen.addr, Some(seen.digest)));
            }
        }
    }
    for p in ops.peer_addresses()? {
        // Only those with a remembered address: this is the fallback for when
        // nothing is advertising, and a peer with no address is one there is
        // no way to call.
        let Some(addr) = p.addr.clone() else { continue };
        if p.key == me || reachable.iter().any(|(k, _, _)| k == &p.key) {
            continue;
        }
        // Backed off, so a device that is simply switched off is not dialled
        // every thirty seconds all day.
        //
        // Through `discovery::backoff`, which is jittered, ceilinged and
        // overflow-safe and has tests saying so. An earlier version computed
        // its own interval here from the attempt count — a second rule,
        // untested, quietly replacing the one that was.
        if let Some(last) = p.last_seen {
            let wait = discovery::backoff(p.attempts, jitter(&p.key)) * 1000;
            if p.attempts > 0 && now().saturating_sub(last) < wait {
                continue;
            }
        }
        reachable.push((p.key, addr, None));
    }

    for (peer, addr, digest) in capture_sync::anti_entropy::select(
        &reachable.iter().map(|(k, _, _)| *k).collect::<Vec<_>>(),
        round,
        capture_sync::anti_entropy::FANOUT,
    )
    .into_iter()
    .filter_map(|k| reachable.iter().find(|(p, _, _)| *p == k).cloned())
    {
        // Nothing to fetch and nothing to offer: the advertisement already
        // said so, and opening the connection to confirm it is the work
        // sealing and the digest exist to avoid.
        if let Some(d) = digest
            && !ops.peer_has_news(&peer, &d)?
        {
            continue;
        }

        match dial(paths, identity, &peer, &addr) {
            Ok(done) => {
                ops.peer_reached(&peer, &addr, now())?;
                if let Some(d) = digest {
                    ops.peer_agreed(&peer, &d)?;
                }
                if done.received > 0 || done.sent > 0 {
                    tracing::info!(
                        peer = %short(&peer),
                        received = done.received,
                        sent = done.sent,
                        "reconciled"
                    );
                }
            }
            Err(e) => {
                let n = ops.peer_missed(&peer)?;
                tracing::debug!(peer = %short(&peer), attempts = n, %e, "peer not reached");
            }
        }
    }

    Ok(())
}

/// Open a session with a peer and run one round of anti-entropy.
fn dial(
    paths: &StorePaths,
    identity: &Identity,
    peer: &[u8; 32],
    addr: &str,
) -> Result<Exchanged, capture_store::Error> {
    let noise = noise_key_for(paths, peer)?;
    let tcp = Tcp::connect(addr).map_err(capture_store::Error::Io)?;
    let mut secure = Secure::initiate(tcp, &identity.noise_secret(), &noise)
        .map_err(|e| capture_store::Error::Usage(format!("handshake refused: {e:?}")))?;

    reconcile(paths, &mut secure)
}

/// Answer a peer that dialled this device.
fn answer(
    paths: &StorePaths,
    identity: &Identity,
    stream: std::net::TcpStream,
) -> Result<(), capture_store::Error> {
    let tcp = Tcp::new(stream).map_err(capture_store::Error::Io)?;

    // Who dialled is established by the handshake rather than claimed before
    // it. A device that has never been a member cannot get this far: the
    // X25519 key it would have to encrypt to lives in the log.
    let mut secure = Secure::accept(tcp, &identity.noise_secret())
        .map_err(|e| capture_store::Error::Usage(format!("handshake refused: {e:?}")))?;

    let Some(caller) = secure.peer() else {
        return Err(capture_store::Error::Usage(
            "the handshake did not establish who called".into(),
        ));
    };

    // The roster check IK makes load-bearing. A *retired* device still holds
    // the log, so it still completes the handshake — what stops it is here.
    // §5.3: on ingesting a retirement, a device refuses new sessions with that
    // origin.
    let (roster, _) = look(paths)?;
    let noise_of = |k: &[u8; 32]| noise_key_for(paths, k).ok();
    let member = roster
        .keys
        .iter()
        .find(|k| noise_of(k).as_ref() == Some(&caller));

    let Some(member) = member.copied() else {
        return Err(capture_store::Error::Usage(format!(
            "refused a session with a device that is not a current member: {}",
            short(&caller)
        )));
    };

    let done = reconcile(paths, &mut secure)?;
    // Alive, but not necessarily reachable at the address it dialled from —
    // that is an ephemeral source port. Where to call it back comes from the
    // advertisement, or from a remembered address, not from this.
    Ops::open(&paths.ops)?.peer_answered(&member, now())?;
    tracing::info!(peer = %short(&member), received = done.received, "answered a peer");
    Ok(())
}

/// Run the exchange, taking the writer lock only for as long as it takes.
fn reconcile<T: capture_sync::transport::Transport>(
    paths: &StorePaths,
    secure: &mut Secure<T>,
) -> Result<Exchanged, capture_store::Error> {
    let mut s = Session::open(paths.clone())?;
    let ops = Ops::open(&paths.ops)?;
    let own = s.identity.public().to_bytes();
    let mut gate = Gate::open(&mut s.log, &mut s.state, &ops, own)?;
    let mut archive = StoreArchive::new(&mut gate, &ops, now());

    let done = exchange(secure, &mut archive)
        .map_err(|e| capture_store::Error::Usage(format!("exchange failed: {e:?}")))?;

    // The log has them; the projection does not until this runs. Without it a
    // round moves every event correctly and no surface shows any of them.
    let folded = archive.settle()?;
    if folded > 0 {
        tracing::debug!(folded, "arrivals folded into the projection");
    }
    Ok(done)
}

/// Membership and what this device holds, without taking the writer lock.
fn look(
    paths: &StorePaths,
) -> Result<(Roster, std::collections::BTreeMap<String, u64>), capture_store::Error> {
    let log = capture_store::log::Log::open_read(&paths.events)?;
    let roster = capture_core::roster::build(&log.all()?);
    let heads = log.heads()?;
    Ok((roster, heads))
}

/// The X25519 key a peer answers a handshake with.
///
/// The log first, because a membership event is where a device says what it
/// is. Then what pairing learned, which is all a freshly joined device has:
/// it cannot dial anybody without a key, the key normally comes from the log,
/// and the log is what it is trying to fetch.
fn noise_key_for(paths: &StorePaths, peer: &[u8; 32]) -> Result<[u8; 32], capture_store::Error> {
    if let Some(k) = from_log(paths, peer)? {
        return Ok(k);
    }
    if let Some(k) = Ops::open(&paths.ops)?.noise_for(peer)? {
        return Ok(k);
    }
    Err(capture_store::Error::Usage(format!(
        "device {} has published no key to open a session with",
        short(peer)
    )))
}

fn from_log(paths: &StorePaths, peer: &[u8; 32]) -> Result<Option<[u8; 32]>, capture_store::Error> {
    let log = capture_store::log::Log::open_read(&paths.events)?;
    for e in log.all()? {
        if !matches!(
            e.header.kind,
            capture_core::event::Kind::DEVICE_ADD | capture_core::event::Kind::ORIGIN_CLAIM
        ) {
            continue;
        }
        let Some(body) = e.body.as_deref() else {
            continue;
        };
        let Ok(claim) = minicbor::decode::<capture_core::body::OriginClaimBody>(body) else {
            continue;
        };
        if &claim.pubkey == peer
            && let Some(noise) = claim.noise
        {
            return Ok(Some(noise));
        }
    }
    Ok(None)
}

fn parse_dev(hex: &str) -> Option<capture_core::Dev> {
    let mut out = [0u8; 32];
    if hex.len() != 64 {
        return None;
    }
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(capture_core::Dev(out))
}

/// Whatever can be said about which network this is.
///
/// The *subnets* this machine sits on, not its own addresses. A host address
/// changes when DHCP reassigns one on the same network, which would rotate the
/// pseudonym for no reason and lose sight of every peer; the prefix does not.
///
/// This being a constant was worse than it looked. §5.2's whole point is that
/// the advertised name differs per network, so that an observer cannot follow
/// a device from a café to an office — and with fixed evidence it did not
/// differ at all. The property was tested by passing two `Network` values to
/// a pure function, and the daemon only ever produced one.
fn network_evidence() -> Vec<u8> {
    let mut nets: Vec<String> = Vec::new();
    for iface in if_addrs::get_if_addrs().unwrap_or_default() {
        if iface.is_loopback() {
            continue;
        }
        let (tag, addr, mask): (&str, Vec<u8>, Vec<u8>) = match iface.addr {
            if_addrs::IfAddr::V4(v4) => ("4", v4.ip.octets().into(), v4.netmask.octets().into()),
            if_addrs::IfAddr::V6(v6) => ("6", v6.ip.octets().into(), v6.netmask.octets().into()),
        };

        // A netmask of all ones describes one address rather than a network,
        // and IPv6 privacy extensions mint a fresh one of those every few
        // hours. Including them would rotate the pseudonym on a network that
        // had not changed, which loses sight of every peer for the same reason
        // moving networks does.
        if mask.iter().all(|b| *b == 0xFF) {
            continue;
        }

        let prefix: Vec<u8> = addr.iter().zip(&mask).map(|(a, m)| a & m).collect();
        nets.push(format!("{tag}:{prefix:?}"));
    }

    // Sorted, because the order interfaces come back in is not promised and
    // two orderings would be two networks as far as the pseudonym is
    // concerned.
    nets.sort();
    nets.dedup();
    nets.join("|").into_bytes()
}

/// Where a peer can actually reach this machine.
///
/// Loopback excluded: it was the only thing advertised before, so anything
/// that found this device over mDNS was told to dial itself. Only the
/// remembered-address fallback ever worked, which is why the two-device runs
/// succeeded and proved less than they appeared to.
fn local_addresses() -> Vec<std::net::IpAddr> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .map(|i| i.ip())
        .collect()
}

/// A per-peer jitter that does not need a source of randomness.
///
/// The point of jitter is that devices do not all retry in the same instant,
/// and a value derived from the peer's own key achieves that while staying a
/// function a test can hold still.
fn jitter(key: &[u8; 32]) -> u64 {
    u64::from_be_bytes(key[..8].try_into().expect("32 bytes has 8"))
}

fn short(k: &[u8; 32]) -> String {
    k[..6].iter().map(|b| format!("{b:02x}")).collect()
}

fn now() -> u64 {
    jiff::Timestamp::now().as_millisecond().max(0) as u64
}
