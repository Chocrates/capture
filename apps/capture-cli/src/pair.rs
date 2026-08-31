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

use capture_store::identity::Identity;
use capture_store::paths::StorePaths;
use capture_store::session::{Admitted, Session};
use capture_store::{Error, Result};

/// Show a code and admit whoever answers it correctly.
pub fn offer(paths: StorePaths) -> Result<()> {
    let offering = capture_store::pairing::Offering::open(&paths)?;
    let shown = offering.shown();

    say!("On the other device, run:  capture pair {shown}");
    say!();
    say!("  code   {shown}");
    say!(
        "  here   this machine, port {}",
        capture_store::pairing::PORT
    );
    say!();
    say!("Waiting. The code stops working after one wrong attempt.");

    let admitted = offering.admit_one(&paths, now(), now)?;

    say!();
    say!("Paired with {}.", short(&admitted));
    say!("It will receive the archive on the next sync.");
    Ok(())
}

/// Type a code shown on another device, and join its roster.
pub fn accept(paths: StorePaths, code: &str, at: Option<&str>) -> Result<()> {
    let digits: Vec<u8> = code.bytes().filter(|b| b.is_ascii_digit()).collect();
    if digits.len() != capture_sync::pairing::CODE_DIGITS {
        return Err(Error::Usage(format!(
            "a pairing code is {} digits",
            capture_sync::pairing::CODE_DIGITS
        )));
    }

    let addr = match at {
        Some(a) if a.contains(':') => a.to_string(),
        Some(a) => format!("{a}:{}", capture_store::pairing::PORT),
        None => {
            return Err(Error::Usage(
                "say where the other device is:  capture pair CODE --at 192.168.1.40".into(),
            ));
        }
    };

    let grant = capture_store::pairing::accept_code(&paths, &digits, &addr, now())?;

    say!("Joined. {} vouched for this device.", short(&grant.pubkey));
    say!(
        "{} device(s) in the roster; the archive arrives on the next sync.",
        grant.members.len().max(1)
    );
    if grant.incarnation.is_none() {
        say!();
        say!("That device is running an older version and did not say which origin");
        say!("it authors under, so this one cannot vouch for it. Update it and");
        say!("pair again — otherwise membership depends on which root wins.");
    }
    if grant.secret.is_none() {
        say!();
        say!("That archive predates sync and has no discovery key yet,");
        say!("so the two will find each other by address rather than by name.");
    }
    Ok(())
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

    let log = capture_store::log::Log::open_read(&paths.events)?;
    let roster = capture_core::roster::build(&log.all()?);

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
    let digest = capture_core::body::readmit_digest(&key, &incarnation, &nonce);
    let sig = ed25519_dalek::Signer::sign(identity.signing(), &digest);

    // If this device is still a member, it writes the re-admission itself
    // rather than handing back a signature to carry.
    //
    // Not a convenience. A retired device cannot open a session — that is what
    // retiring it did — so a re-admission it authored has no way to reach the
    // devices that refuse it, and the remedy cannot travel through the refusal
    // it exists to undo. Authored here it is an ordinary event from a member,
    // and it replicates like one.
    let log = capture_store::log::Log::open_read(&paths.events)?;
    let mut roster = capture_core::roster::build(&log.all()?);
    roster.honour_self(&identity.public().to_bytes());
    drop(log);

    if roster.keys.contains(&identity.public().to_bytes()) {
        let subject = Admitted {
            pubkey: key,
            incarnation,
            // Not known here, and not needed: an origin claim or a pairing
            // publishes it, and this device already holds one for a peer it
            // had admitted before. Zero would be a claim, so the roster's own
            // record is left to stand.
            noise: [0u8; 32],
        };
        let mut s = Session::open(paths.clone())?;
        s.readmit_with(
            &subject,
            &identity.public().to_bytes(),
            &sig.to_bytes(),
            nonce,
        )?;

        say!("Re-admitted {}.", short(&key));
        say!("It is a member again as soon as this reaches it.");
        say!();
        say!("Nothing to carry back: this device is still a member, so it wrote");
        say!("the re-admission itself. A retired device cannot open a session,");
        say!("so one it wrote could never have reached the devices refusing it.");
        return Ok(());
    }

    say!("This device is retired too, so it cannot write the re-admission.");
    say!("On the other device:");
    say!();
    say!(
        "  capture readmit --cosign {} --by {}",
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
        say!("  capture cosign {}", subject_blob(&identity));
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
    let digest =
        capture_core::body::readmit_digest(&subject.pubkey, &subject.incarnation, &[0; 16]);
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

fn short(k: &[u8; 32]) -> String {
    k[..6].iter().map(|b| format!("{b:02x}")).collect()
}

fn now() -> u64 {
    jiff::Timestamp::now().as_millisecond().max(0) as u64
}
