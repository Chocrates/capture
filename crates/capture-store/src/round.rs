//! One round of sync: advertise, find, dial, reconcile.
//!
//! # Why this is here rather than in the daemon
//!
//! It was in the daemon, which was fine while the daemon was the only thing
//! that ran it. A phone has no daemon — the platform does not permit a
//! resident process, and sync happens in a `WorkManager` job that is handed a
//! few seconds and then killed. That job has to do exactly what a tick does.
//!
//! Written twice, the second copy is the one nobody exercises: both would look
//! correct in isolation, and the divergence would show up as two devices that
//! sync perfectly with their own kind and imperfectly with each other. That is
//! the same argument `pairing` is in this crate for, and the same one the
//! Kotlin gate makes in another language.
//!
//! So the round lives here and the callers differ only in what wakes them.
//!
//! # What is still the caller's
//!
//! When to run, and what to hold while running. The daemon has a timer, a
//! resume signal and a network-change signal; Android has constraints and a
//! multicast lock it must hold or receive nothing. Neither belongs here.

use std::collections::BTreeMap;

use capture_core::roster::Roster;
use capture_sync::anti_entropy::{Exchanged, exchange};
#[cfg(feature = "mdns")]
use capture_sync::discovery::{self, Network};
use capture_sync::session::Session as Secure;
use capture_sync::tcp::Tcp;

use crate::gate::Gate;
use crate::identity::Identity;
use crate::ops::Ops;
use crate::paths::StorePaths;
use crate::session::Session;
use crate::sync::StoreArchive;
use crate::{Error, Result};

#[cfg(feature = "mdns")]
use capture_sync::mdns::Beacon;

/// One round: advertise what this device holds, and reconcile with whoever is
/// reachable.
///
/// Needs a responder, and so needs the feature. Everything below it does not:
/// reconciling with a peer whose address is already known is the fallback for
/// every network that blocks multicast, and it is also how the two-device
/// tests drive a sync without a network to discover anything on.
#[cfg(feature = "mdns")]
pub fn tick(
    paths: &StorePaths,
    identity: &Identity,
    beacon: &mut Option<Beacon>,
    round: u64,
    port: u16,
) -> Result<()> {
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
pub fn dial(
    paths: &StorePaths,
    identity: &Identity,
    peer: &[u8; 32],
    addr: &str,
) -> Result<Exchanged> {
    let noise = noise_key_for(paths, peer)?;
    let tcp = Tcp::connect(addr).map_err(Error::Io)?;
    let mut secure = Secure::initiate(tcp, &identity.noise_secret(), &noise)
        .map_err(|e| Error::Usage(format!("handshake refused: {e:?}")))?;

    reconcile(paths, &mut secure)
}

/// Answer a peer that dialled this device.
pub fn answer(paths: &StorePaths, identity: &Identity, stream: std::net::TcpStream) -> Result<()> {
    let tcp = Tcp::new(stream).map_err(Error::Io)?;

    // Who dialled is established by the handshake rather than claimed before
    // it. A device that has never been a member cannot get this far: the
    // X25519 key it would have to encrypt to lives in the log.
    let mut secure = Secure::accept(tcp, &identity.noise_secret())
        .map_err(|e| Error::Usage(format!("handshake refused: {e:?}")))?;

    let Some(caller) = secure.peer() else {
        return Err(Error::Usage(
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
        return Err(Error::Usage(format!(
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
pub fn reconcile<T: capture_sync::transport::Transport>(
    paths: &StorePaths,
    secure: &mut Secure<T>,
) -> Result<Exchanged> {
    let mut s = Session::open(paths.clone())?;
    let ops = Ops::open(&paths.ops)?;
    let own = s.identity.public().to_bytes();
    let mut gate = Gate::open(&mut s.log, &mut s.state, &ops, own)?;
    let mut archive = StoreArchive::new(&mut gate, &ops, now());

    let done = exchange(secure, &mut archive)
        .map_err(|e| Error::Usage(format!("exchange failed: {e:?}")))?;

    // The log has them; the projection does not until this runs. Without it a
    // round moves every event correctly and no surface shows any of them.
    let folded = archive.settle()?;
    if folded > 0 {
        tracing::debug!(folded, "arrivals folded into the projection");
    }
    Ok(done)
}

/// Membership and what this device holds, without taking the writer lock.
pub fn look(paths: &StorePaths) -> Result<(Roster, BTreeMap<String, u64>)> {
    let log = crate::log::Log::open_read(&paths.events)?;
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
pub fn noise_key_for(paths: &StorePaths, peer: &[u8; 32]) -> Result<[u8; 32]> {
    if let Some(k) = from_log(paths, peer)? {
        return Ok(k);
    }
    if let Some(k) = Ops::open(&paths.ops)?.noise_for(peer)? {
        return Ok(k);
    }
    Err(Error::Usage(format!(
        "device {} has published no key to open a session with",
        short(peer)
    )))
}

fn from_log(paths: &StorePaths, peer: &[u8; 32]) -> Result<Option<[u8; 32]>> {
    let log = crate::log::Log::open_read(&paths.events)?;
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

pub fn parse_dev(hex: &str) -> Option<capture_core::Dev> {
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
pub fn network_evidence() -> Vec<u8> {
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
pub fn local_addresses() -> Vec<std::net::IpAddr> {
    if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter(|i| !i.is_loopback())
        .map(|i| i.ip())
        .collect()
}

/// A per-peer jitter that does not need a source of randomness.
#[cfg(feature = "mdns")]
///
/// The point of jitter is that devices do not all retry in the same instant,
/// and a value derived from the peer's own key achieves that while staying a
/// function a test can hold still.
fn jitter(key: &[u8; 32]) -> u64 {
    u64::from_be_bytes(key[..8].try_into().expect("32 bytes has 8"))
}

pub fn short(k: &[u8; 32]) -> String {
    k[..6].iter().map(|b| format!("{b:02x}")).collect()
}

pub fn now() -> u64 {
    jiff::Timestamp::now().as_millisecond().max(0) as u64
}
