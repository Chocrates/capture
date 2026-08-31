//! Admitting a device, and joining a roster.
//!
//! One physical gesture: six digits shown on one device and typed into the
//! other. Everything after that is automatic, and every other device learns
//! the newcomer by folding the admission rather than by being paired with it
//! (§5.3) — which is what makes this O(1) gestures rather than O(n²).
//!
//! # Why the CLI and not the daemon
//!
//! Pairing is a deliberate act with a person watching. It is a one-shot
//! synchronous exchange with a window that closes, and putting it in a
//! background loop would mean the window's lifetime was a scheduling detail
//! rather than something the person can see.

use std::net::TcpListener;
use std::time::Duration;

use brain_store::identity::Identity;
use brain_store::ops::Ops;
use brain_store::paths::StorePaths;
use brain_store::session::{Admitted, Session};
use brain_store::{Error, Result};
use brain_sync::pairing::{Grant, Offer, admit, code_from, join};
use brain_sync::tcp::Tcp;

/// How long the window stays open, in milliseconds.
///
/// Long enough to walk to the other device, short enough that a code left on
/// a screen is not an open door. It closes on the first failed attempt in any
/// case — a million online guesses is a different proposition from a million
/// offline ones, and only if the code stands still.
const WINDOW_MS: u64 = 180 * 1000;

/// The port the pairing half listens on.
///
/// Not the sync port: pairing is a separate, deliberate exchange with a window
/// that closes, and running it on the port a daemon is already bound to would
/// mean one could not happen while the other was.
const PORT: u16 = 41823;

/// Show a code and admit whoever answers it correctly.
pub fn offer(paths: StorePaths) -> Result<()> {
    let identity = Identity::load(&paths)?;
    let code = code_from(brain_store::entropy::pairing_entropy()?);
    let shown = String::from_utf8_lossy(&code).to_string();

    let listener = TcpListener::bind(("0.0.0.0", PORT))?;
    listener.set_nonblocking(false)?;

    say!("On the other device, run:  brain pair {shown}");
    say!();
    say!("  code   {shown}");
    say!("  here   {}", addresses(PORT));
    say!();
    say!("Waiting. The code stops working after one wrong attempt.");

    // A read timeout rather than a loop with a sleep: the window is a real
    // deadline and the person is standing there.
    listener.set_nonblocking(true)?;
    let started = now();
    let stream = loop {
        match listener.accept() {
            Ok((s, _)) => break s,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if now().saturating_sub(started) > WINDOW_MS {
                    return Err(Error::Usage(
                        "nobody answered before the code expired".into(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(Error::Io(e)),
        }
    };
    stream.set_nonblocking(false)?;

    let tcp = Tcp::new(stream)?;
    let mine = grant_from(&paths, &identity)?;

    // The offer is captured out of the callback so the admission can be
    // written *after* the exchange proves the other side knew the code.
    let mut learned: Option<Offer> = None;
    let offered = admit(tcp, &code, |o| {
        learned = Some(o.clone());
        mine.clone()
    })
    .map_err(|e| Error::Usage(format!("{e}")))?;

    let subject = Admitted {
        pubkey: offered.pubkey,
        incarnation: offered.incarnation,
        noise: offered.noise,
    };

    let mut s = Session::open(paths.clone())?;
    s.admit(&subject)?;

    // What makes "a device was added that you did not expect" computable at
    // all. Without it every admission in the log looks alike, including the
    // ones this device performed itself.
    let ops = Ops::open(&paths.ops)?;
    ops.paired(&offered.pubkey, now())?;
    ops.learned_noise(&offered.pubkey, &offered.noise)?;

    say!();
    say!("Paired with {}.", short(&offered.pubkey));
    say!("It will receive the archive on the next sync.");
    Ok(())
}

/// Type a code shown on another device, and join its roster.
pub fn accept(paths: StorePaths, code: &str, at: Option<&str>) -> Result<()> {
    let identity = Identity::load(&paths)?;
    let digits: Vec<u8> = code.bytes().filter(|b| b.is_ascii_digit()).collect();
    if digits.len() != brain_sync::pairing::CODE_DIGITS {
        return Err(Error::Usage(format!(
            "a pairing code is {} digits",
            brain_sync::pairing::CODE_DIGITS
        )));
    }

    let addr = match at {
        Some(a) if a.contains(':') => a.to_string(),
        Some(a) => format!("{a}:{PORT}"),
        None => {
            return Err(Error::Usage(
                "say where the other device is:  brain pair CODE --at 192.168.1.40".into(),
            ));
        }
    };

    let tcp = Tcp::connect(&addr)?;
    let offer = Offer {
        pubkey: identity.public().to_bytes(),
        incarnation: identity.incarnation().0,
        noise: identity.noise_public(),
    };

    let grant = join(tcp, &digits, &offer).map_err(|e| Error::Usage(format!("{e}")))?;

    // The admitting device's key, so this one has a peer it can actually dial.
    // Without it the newcomer holds a roster it cannot talk to.
    let ops = Ops::open(&paths.ops)?;
    ops.paired(&grant.pubkey, now())?;
    // The address a *sync* will use, not the one pairing just used. They are
    // different ports, and recording the pairing port would leave the newcomer
    // with a remembered address that never answers.
    let host = addr.split(':').next().unwrap_or("127.0.0.1");
    ops.peer_reached(
        &grant.pubkey,
        &format!("{host}:{}", brain_sync::discovery::sync_port()),
        now(),
    )?;
    // Without this the newcomer cannot dial anybody: opening a session needs
    // the peer's X25519 key, that key normally comes from the log, and the log
    // is the thing it has not got yet. The grant carries it to break exactly
    // that deadlock.
    ops.learned_noise(&grant.pubkey, &grant.noise)?;

    // The other half of the mutual exchange. Each device has a root of its
    // own, and only one root survives the merge — so a device that only ever
    // *was* admitted is a device whose admitter may turn out not to be a
    // member of the combined roster, leaving everything it writes
    // unverifiable here. Admitting it back makes membership hold whichever
    // root wins.
    match grant.incarnation {
        Some(incarnation) => {
            let mut s = Session::open(paths.clone())?;
            s.admit(&Admitted {
                pubkey: grant.pubkey,
                incarnation,
                noise: grant.noise,
            })?;
        }
        None => {
            // A grant from a build that predates this. Say so rather than
            // proceeding quietly: the pairing half-works, and the failure it
            // leads to is silent and permanent.
            say!("The other device is running an older version and did not say");
            say!("which origin it authors under, so this device cannot vouch for");
            say!("it. Update it and pair again.");
        }
    }

    say!("Joined. {} vouched for this device.", short(&grant.pubkey));
    say!(
        "{} device(s) in the roster; the archive arrives on the next sync.",
        grant.members.len().max(1)
    );
    if grant.secret.is_none() {
        say!();
        say!("That archive predates sync and has no discovery key yet,");
        say!("so the two will find each other by address rather than by name.");
    }
    Ok(())
}

/// What this device hands a newcomer.
fn grant_from(paths: &StorePaths, identity: &Identity) -> Result<Grant> {
    let log = brain_store::log::Log::open_read(&paths.events)?;
    let roster = brain_core::roster::build(&log.all()?);

    Ok(Grant {
        pubkey: identity.public().to_bytes(),
        noise: identity.noise_public(),
        members: roster.keys.iter().map(|k| (*k).into()).collect(),
        secret: roster.secret,
        incarnation: Some(identity.incarnation().0),
    })
}

/// Whatever addresses this machine might be reached on.
fn addresses(port: u16) -> String {
    // Best effort, and only ever shown to a person who can also read it off
    // their own network settings.
    format!("this machine, port {port}")
}

fn short(k: &[u8; 32]) -> String {
    k[..6].iter().map(|b| format!("{b:02x}")).collect()
}

fn now() -> u64 {
    jiff::Timestamp::now().as_millisecond().max(0) as u64
}

/// Stop accepting what a device writes from here on.
///
/// The remedy for a device that was lost or stolen. It retires the **key**,
/// not an origin: retiring an origin would let the device mint a fresh
/// incarnation on its next restart and walk straight back in.
///
/// What it does *not* do is delete anything. A retired device's past events
/// were legitimately authored and stay valid — discarding them would mean
/// losing real notes, which is a worse outcome than the access this closes.
pub fn retire(paths: StorePaths, key: &str, force: bool) -> Result<()> {
    let identity = Identity::load(&paths)?;
    let me = identity.public().to_bytes();

    let log = brain_store::log::Log::open_read(&paths.events)?;
    let roster = brain_core::roster::build(&log.all()?);

    let wanted = key.to_ascii_lowercase();
    let matches: Vec<[u8; 32]> = roster
        .keys
        .iter()
        .copied()
        .filter(|k| short(k).starts_with(&wanted))
        .collect();

    let subject = match matches.as_slice() {
        [] => {
            return Err(Error::NotFound(format!(
                "no device in the roster matches {key}"
            )));
        }
        [one] => *one,
        many => {
            say!("{} devices start with {key}:", many.len());
            for k in many {
                say!("  {}", short(k));
            }
            return Err(Error::Usage("give more of the key".into()));
        }
    };

    if subject == me {
        // Retiring yourself is a thing you can mean, and it is also what a
        // mistyped key most often turns out to be.
        say!("{} is this device.", short(&subject));
        say!("Retiring it stops every *other* device accepting what this one");
        say!("writes from now on. This device will go on working locally.");
    }
    if roster.keys.len() == 1 {
        return Err(Error::Usage(
            "this is the only device in the roster, and a roster with nobody in it can never \
             admit anyone again"
                .into(),
        ));
    }

    if !force {
        say!(
            "This will stop {} being accepted by this archive.",
            short(&subject)
        );
        say!();
        say!("  Its past events stay — they were legitimately authored.");
        say!("  Anything it writes *after* the point this device has seen is refused.");
        say!("  Other devices learn of it as they sync.");
        say!();
        say!("Re-run with --yes to go ahead.");
        return Ok(());
    }

    let mut s = Session::open(paths.clone())?;
    let id = s.retire(&subject, None)?;

    say!("Retired {}.", short(&subject));
    say!(
        "  {}",
        id.0[..4]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    say!();
    say!("If it turns out to have written more than this device had seen, retire");
    say!("it again once you know: the later head replaces this one.");
    Ok(())
}

// ------------------------------------------------------------ readmission ---
//
// The way back from a lockout, and the only event whose validity does not
// descend from current roster membership. It needs two signatures because any
// member may retire any member: one compromised or malfunctioning device can
// retire every other, after which `device_add` cannot help — it requires a
// signature from a current member, and there are none left.
//
// Three steps rather than one, because the two signatures come from two
// machines and that is the point. Forging this requires compromising two
// devices *and* being in the room with them, which is a materially higher bar
// than the single compromised device that caused the lockout.

/// What this device needs another to vouch for.
fn subject_blob(identity: &Identity) -> String {
    format!(
        "{}.{}",
        hexed(&identity.public().to_bytes()),
        hexed(&identity.incarnation().0)
    )
}

fn hexed(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect()
}

/// Vouch for another device's re-admission.
///
/// Run on a device that still works, with the blob the locked-out one printed.
/// It signs a statement about *which device* is being re-admitted and nothing
/// else — this does not admit anybody by itself, and cannot: the signature is
/// useless without a second one from the device it names.
pub fn cosign(paths: StorePaths, blob: &str) -> Result<()> {
    let identity = Identity::load(&paths)?;

    let (key, incarnation) = blob
        .split_once('.')
        .ok_or_else(|| Error::Usage("that is not a re-admission request".into()))?;
    let (Some(key), Some(incarnation)) = (unhex(key), unhex(incarnation)) else {
        return Err(Error::Usage("that is not a re-admission request".into()));
    };
    let (Ok(key), Ok(incarnation)) = (
        <[u8; 32]>::try_from(key.as_slice()),
        <[u8; 16]>::try_from(incarnation.as_slice()),
    ) else {
        return Err(Error::Usage("that is not a re-admission request".into()));
    };

    if key == identity.public().to_bytes() {
        return Err(Error::Usage(
            "a device cannot vouch for itself — that is one device's say-so written twice".into(),
        ));
    }

    // A fixed nonce, so the two devices do not have to agree one first. The
    // digest is domain-separated and says only which device is meant, so a
    // second copy of the signature re-admits the same device, which is what it
    // already says.
    let nonce = [0u8; 16];
    let digest = brain_core::body::readmit_digest(&key, &incarnation, &nonce);
    let sig = ed25519_dalek::Signer::sign(identity.signing(), &digest);

    say!("On the other device:");
    say!();
    say!(
        "  brain readmit --cosign {} --by {}",
        hexed(&sig.to_bytes()),
        hexed(&identity.public().to_bytes())
    );
    say!();
    say!("This vouches for that device and nothing else. On its own it admits");
    say!("nobody: it is useless without the signature of the device it names.");
    Ok(())
}

/// Re-admit this device, with another's signature.
pub fn readmit(paths: StorePaths, cosign: Option<&str>, by: Option<&str>) -> Result<()> {
    let identity = Identity::load(&paths)?;

    let (Some(cosign), Some(by)) = (cosign, by) else {
        say!("This device needs another to vouch for it.");
        say!();
        say!("On a device that still works, run:");
        say!();
        say!("  brain cosign {}", subject_blob(&identity));
        say!();
        say!("then bring its answer back here.");
        return Ok(());
    };

    let (Some(sig), Some(signer)) = (unhex(cosign), unhex(by)) else {
        return Err(Error::Usage("that is not a co-signature".into()));
    };
    let (Ok(sig), Ok(signer)) = (
        <[u8; 64]>::try_from(sig.as_slice()),
        <[u8; 32]>::try_from(signer.as_slice()),
    ) else {
        return Err(Error::Usage("that is not a co-signature".into()));
    };

    let subject = Admitted {
        pubkey: identity.public().to_bytes(),
        incarnation: identity.incarnation().0,
        noise: identity.noise_public(),
    };

    // Checked here as well as in the fold, so a mistyped signature is a
    // message rather than an event that every device quietly refuses.
    let digest = brain_core::body::readmit_digest(&subject.pubkey, &subject.incarnation, &[0; 16]);
    let ok = ed25519_dalek::VerifyingKey::from_bytes(&signer)
        .ok()
        .and_then(|k| {
            k.verify_strict(&digest, &ed25519_dalek::Signature::from_bytes(&sig))
                .ok()
        })
        .is_some();
    if !ok {
        return Err(Error::Usage(
            "that signature does not vouch for this device — check it was copied whole".into(),
        ));
    }

    let mut s = Session::open(paths.clone())?;
    s.readmit_with(&subject, &signer, &sig, [0; 16])?;

    say!("Re-admitted {}.", short(&subject.pubkey));
    say!("Other devices accept it again as they sync.");
    Ok(())
}
