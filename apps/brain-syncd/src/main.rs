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

use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

use brain_core::roster::Roster;
use brain_store::gate::Gate;
use brain_store::identity::Identity;
use brain_store::ops::Ops;
use brain_store::paths::StorePaths;
use brain_store::session::Session;
use brain_store::sync::StoreArchive;
use brain_sync::anti_entropy::{Exchanged, exchange};
use brain_sync::discovery::{self, Network};
use brain_sync::mdns::Beacon;
use brain_sync::session::Session as Secure;
use brain_sync::tcp::Tcp;

/// How often to look around when nothing is happening.
const IDLE: Duration = Duration::from_secs(30);

/// What to say when asked, rather than starting.
const USAGE: &str = "\
brain-syncd — find paired devices and reconcile with them

    brain-syncd            run in the foreground until stopped
    brain-syncd --help     this

It takes no other options. Where the store lives comes from $BRAIN_HOME or the
usual place, and the port from $BRAIN_SYNC_PORT — which exists for the one case
of two stores on one machine, and is otherwise best left alone.

It reads the store without the writer lock and takes it only for as long as it
takes to hand arriving events over, so it can run alongside the overlay and the
CLI without contending with either.

Sync does nothing until a second device exists. To add one:  brain pair
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
                .unwrap_or_else(|_| "brain_syncd=info,brain_sync=info".into()),
        )
        .init();

    if let Err(e) = run() {
        tracing::error!(%e, "the sync daemon stopped");
        std::process::exit(1);
    }
}

fn run() -> Result<(), brain_store::Error> {
    let paths = StorePaths::resolve().ok_or_else(|| {
        brain_store::Error::Usage(
            "cannot work out where the store should live: no home directory, and \
             $BRAIN_HOME is not set"
                .into(),
        )
    })?;
    let identity = brain_store::store::open(&paths)?;

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
    let mut beacon = match Beacon::new() {
        Ok(b) => Some(b),
        Err(e) => {
            tracing::info!(%e, "no mDNS on this network; falling back to remembered addresses");
            None
        }
    };

    let mut round = 0u64;
    loop {
        if let Err(e) = tick(&paths, &identity, &mut beacon, round, port) {
            // A round that fails is a round. The next one may find a different
            // network, a woken peer, or a released lock.
            tracing::warn!(%e, "sync round did not complete");
        }
        round += 1;

        // Drain anything that arrived while the round ran.
        while let Ok(arrival) = rx.recv_timeout(IDLE) {
            match arrival {
                Arrival::Dialled(stream) => {
                    if let Err(e) = answer(&paths, &identity, stream) {
                        tracing::info!(%e, "a peer connection ended");
                    }
                }
            }
        }
    }
}

enum Arrival {
    Dialled(std::net::TcpStream),
}

/// One round: advertise what this device holds, and reconcile with whoever is
/// reachable.
fn tick(
    paths: &StorePaths,
    identity: &Identity,
    beacon: &mut Option<Beacon>,
    round: u64,
    port: u16,
) -> Result<(), brain_store::Error> {
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

    let heads_pairs: Vec<(brain_core::Dev, u64)> = heads
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
        if p.attempts > 0 && !round.is_multiple_of(u64::from(1 + p.attempts.min(8))) {
            continue;
        }
        reachable.push((p.key, addr, None));
    }

    for (peer, addr, digest) in brain_sync::anti_entropy::select(
        &reachable.iter().map(|(k, _, _)| *k).collect::<Vec<_>>(),
        round,
        brain_sync::anti_entropy::FANOUT,
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
) -> Result<Exchanged, brain_store::Error> {
    let noise = noise_key_for(paths, peer)?;
    let tcp = Tcp::connect(addr).map_err(brain_store::Error::Io)?;
    let mut secure = Secure::initiate(tcp, &identity.noise_secret(), &noise)
        .map_err(|e| brain_store::Error::Usage(format!("handshake refused: {e:?}")))?;

    reconcile(paths, &mut secure)
}

/// Answer a peer that dialled this device.
fn answer(
    paths: &StorePaths,
    identity: &Identity,
    stream: std::net::TcpStream,
) -> Result<(), brain_store::Error> {
    let tcp = Tcp::new(stream).map_err(brain_store::Error::Io)?;

    // Who dialled is established by the handshake rather than claimed before
    // it. A device that has never been a member cannot get this far: the
    // X25519 key it would have to encrypt to lives in the log.
    let mut secure = Secure::accept(tcp, &identity.noise_secret())
        .map_err(|e| brain_store::Error::Usage(format!("handshake refused: {e:?}")))?;

    let Some(caller) = secure.peer() else {
        return Err(brain_store::Error::Usage(
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
        return Err(brain_store::Error::Usage(format!(
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
fn reconcile<T: brain_sync::transport::Transport>(
    paths: &StorePaths,
    secure: &mut Secure<T>,
) -> Result<Exchanged, brain_store::Error> {
    let mut s = Session::open(paths.clone())?;
    let ops = Ops::open(&paths.ops)?;
    let mut gate = Gate::open(&mut s.log, &mut s.state, &ops)?;
    let mut archive = StoreArchive::new(&mut gate, &ops, now());

    let done = exchange(secure, &mut archive)
        .map_err(|e| brain_store::Error::Usage(format!("exchange failed: {e:?}")))?;

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
) -> Result<(Roster, std::collections::BTreeMap<String, u64>), brain_store::Error> {
    let log = brain_store::log::Log::open_read(&paths.events)?;
    let roster = brain_core::roster::build(&log.all()?);
    let heads = log.heads()?;
    Ok((roster, heads))
}

/// The X25519 key a peer answers a handshake with.
///
/// The log first, because a membership event is where a device says what it
/// is. Then what pairing learned, which is all a freshly joined device has:
/// it cannot dial anybody without a key, the key normally comes from the log,
/// and the log is what it is trying to fetch.
fn noise_key_for(paths: &StorePaths, peer: &[u8; 32]) -> Result<[u8; 32], brain_store::Error> {
    if let Some(k) = from_log(paths, peer)? {
        return Ok(k);
    }
    if let Some(k) = Ops::open(&paths.ops)?.noise_for(peer)? {
        return Ok(k);
    }
    Err(brain_store::Error::Usage(format!(
        "device {} has published no key to open a session with",
        short(peer)
    )))
}

fn from_log(paths: &StorePaths, peer: &[u8; 32]) -> Result<Option<[u8; 32]>, brain_store::Error> {
    let log = brain_store::log::Log::open_read(&paths.events)?;
    for e in log.all()? {
        if !matches!(
            e.header.kind,
            brain_core::event::Kind::DEVICE_ADD | brain_core::event::Kind::ORIGIN_CLAIM
        ) {
            continue;
        }
        let Some(body) = e.body.as_deref() else {
            continue;
        };
        let Ok(claim) = minicbor::decode::<brain_core::body::OriginClaimBody>(body) else {
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

fn parse_dev(hex: &str) -> Option<brain_core::Dev> {
    let mut out = [0u8; 32];
    if hex.len() != 64 {
        return None;
    }
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(brain_core::Dev(out))
}

/// Whatever can be said about which network this is.
fn network_evidence() -> Vec<u8> {
    // Deliberately coarse. What matters is that it is stable while attached to
    // one network and different on another; getting it exactly right is a
    // platform question, and getting it wrong costs discovery latency rather
    // than correctness.
    let mut out = Vec::new();
    for iface in local_addresses() {
        out.extend_from_slice(iface.to_string().as_bytes());
    }
    out
}

fn local_addresses() -> Vec<std::net::IpAddr> {
    // A placeholder until the daemon enumerates interfaces properly. Binding
    // to all interfaces still works; what suffers is the advertised address.
    vec![std::net::IpAddr::from([127, 0, 0, 1])]
}

fn short(k: &[u8; 32]) -> String {
    k[..6].iter().map(|b| format!("{b:02x}")).collect()
}

fn now() -> u64 {
    jiff::Timestamp::now().as_millisecond().max(0) as u64
}
