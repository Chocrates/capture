//! Checking that the archive still says what it was written to say.
//!
//! # What is actually checked
//!
//! Four things, per event, over the whole log:
//!
//! 1. **The identity matches the header.** An id is `blake3` over the canonical
//!    header, so recomputing it and comparing against the key the row is stored
//!    under catches a header that has been altered since it was written.
//! 2. **The signature verifies** under the key the roster says the origin
//!    authors with.
//! 3. **The body hashes to its commitment**, where a body is still present. A
//!    redacted event has nothing to hash; that is a known state, not a fault.
//! 4. **Each origin's chain is intact** — sequence numbers contiguous from one,
//!    and every event linking to its predecessor.
//!
//! Nothing here consults the projection. The projection is disposable and can
//! be rebuilt from the log at any time, so its agreeing with the log proves
//! nothing about the log; only the log itself is worth verifying.
//!
//! # Why this is a separate command and not part of `status`
//!
//! It reads and hashes every event. On a large archive that is seconds, not
//! milliseconds, and a status page that took seconds would stop being run.
//! `status` therefore reports what it checked and names this command for the
//! rest, rather than implying an integrity guarantee it never established.
//!
//! # Refusals are counted, not failed on
//!
//! Events that failed authentication were never part of the log — refusing them
//! is the system working. Reporting them as an integrity failure would mean a
//! store that had once been offered a forged event could never verify clean
//! again, which trains the reader to ignore the one signal that matters.

use std::collections::BTreeMap;

use capture_core::{
    Dev,
    envelope::{ChainBreak, Envelope, Rejection, Verdict, verify_chain},
    id::BodyHash,
    roster,
};
use capture_store::{Error, Result, StorePaths, log::Log, ops::Ops, store};

use crate::json::{Json, View, answer, arr, obj};

/// Something wrong with a stored event.
struct Fault {
    origin: String,
    seq: u64,
    what: String,
}

/// Check every event in the log.
pub fn run(paths: &StorePaths, json: bool) -> Result<()> {
    if !store::exists(paths) {
        return Err(Error::StoreMissing(paths.root.clone()));
    }

    // Read-only, and independent of the writer. A check that needs the system
    // to be healthy before it will run is useless at the moment it is wanted.
    let log = Log::open_read(&paths.events)?;
    let rows = log.rows()?;
    let roster = roster::build(&rows.iter().map(|(_, e)| e.clone()).collect::<Vec<_>>());

    let mut faults: Vec<Fault> = Vec::new();
    let mut by_origin: BTreeMap<Dev, Vec<Envelope>> = BTreeMap::new();
    let mut redacted = 0usize;
    let mut unverifiable = 0usize;

    for (stored, e) in &rows {
        let origin = short(&e.header.dev);
        let seq = e.header.seq;

        // 1. The row's key against the header it holds.
        let computed = e.header.id();
        if computed != *stored {
            faults.push(Fault {
                origin: origin.clone(),
                seq,
                what: format!(
                    "stored under {} but its header produces {}",
                    hex(&stored.0[..8]),
                    hex(&computed.0[..8])
                ),
            });
            // Everything below is a statement about an event whose identity is
            // already in doubt, so reporting more about it would be noise.
            continue;
        }

        if e.body.is_none() {
            redacted += 1;
        }

        // 2 and 3. `verify` does both, so calling it and then hashing the body
        // again would report one damaged body as two separate faults.
        match roster.key_for(&e.header.dev) {
            Some(key) => match e.verify(key) {
                Verdict::Authentic => {}
                Verdict::Rejected(r) => faults.push(Fault {
                    origin: origin.clone(),
                    seq,
                    what: describe(r),
                }),
            },
            None => {
                // No key for this origin. Not corruption: the claim binding the
                // origin to a key may simply not have arrived, and calling that
                // an integrity failure would make a mid-sync store look damaged.
                //
                // The body is still checked. Hashing needs no key, so declining
                // to check it here would leave every event from an unclaimed
                // origin completely unexamined.
                unverifiable += 1;
                if let Some(body) = &e.body
                    && BodyHash::of(body) != e.header.bhash
                {
                    faults.push(Fault {
                        origin: origin.clone(),
                        seq,
                        what: "body does not match the hash its header commits to".into(),
                    });
                }
            }
        }

        by_origin.entry(e.header.dev).or_default().push(e.clone());
    }

    // 4. Each origin's chain.
    for (dev, mut run) in by_origin {
        run.sort_by_key(|e| e.header.seq);
        if let Some(first) = run.first()
            && first.header.seq != 1
        {
            faults.push(Fault {
                origin: short(&dev),
                seq: first.header.seq,
                what: format!("first event is sequence {}, not 1", first.header.seq),
            });
        }
        if let Err(b) = verify_chain(&run) {
            faults.push(match b {
                ChainBreak::Gap { after } => Fault {
                    origin: short(&dev),
                    seq: after,
                    what: format!("sequence jumps after {after}; an event is missing"),
                },
                ChainBreak::Broken { at } => Fault {
                    origin: short(&dev),
                    seq: at,
                    what: format!("event {at} does not link to the one before it"),
                },
            });
        }
    }

    let refused = Ops::open_read(&paths.ops)?.rejected_total()?;

    let failed = faults.len();
    answer(
        &Verified {
            checked: rows.len(),
            faults,
            redacted,
            unverifiable,
            refused,
        },
        json,
    );

    if failed == 0 {
        Ok(())
    } else {
        // Its own exit code. A damaged archive is not a mistyped command, and a
        // script that cannot tell them apart will treat both as retryable.
        Err(Error::Integrity(format!(
            "{failed} event(s) failed verification"
        )))
    }
}

/// What a pass over the log found.
struct Verified {
    checked: usize,
    faults: Vec<Fault>,
    redacted: usize,
    unverifiable: usize,
    refused: u64,
}

impl View for Verified {
    fn json(&self) -> Json {
        let Verified {
            checked,
            faults,
            redacted,
            unverifiable,
            refused,
        } = self;
        obj([
            ("checked", checked.into()),
            (
                "faults",
                arr(faults.iter().map(|Fault { origin, seq, what }| {
                    obj([
                        ("origin", origin.into()),
                        ("seq", seq.into()),
                        ("problem", what.into()),
                    ])
                })),
            ),
            ("redacted", redacted.into()),
            ("unverifiable", unverifiable.into()),
            ("refused", refused.into()),
        ])
    }

    fn prose(&self) {
        let Verified {
            checked,
            faults,
            redacted,
            unverifiable,
            refused,
        } = self;
        report(*checked, faults, *redacted, *unverifiable, *refused);
    }
}

fn report(checked: usize, faults: &[Fault], redacted: usize, unverifiable: usize, refused: u64) {
    say!("checked {checked} event(s)");

    if redacted > 0 {
        // Named as expected, because it is. A redacted event keeps its header
        // and its signature; only the payload is gone, which is the point.
        say!("  {redacted} redacted — no body to hash, which is the intended state");
    }
    if unverifiable > 0 {
        say!(
            "  {unverifiable} from an origin with no known key — the claim binding it may not \
             have arrived yet"
        );
    }
    if refused > 0 {
        // Counted, never failed on. These were refused at the door and hold no
        // position; treating them as damage would mean a store that was once
        // offered a forged event could never verify clean again.
        say!("  {refused} refused at ingest and never admitted");
    }

    say!();
    if faults.is_empty() {
        // Said plainly, and scoped to what was actually examined.
        say!("Every id, signature, body hash and origin chain checks out.");
        return;
    }

    say!("Damaged");
    for f in faults {
        say!("  {} seq {}  {}", f.origin, f.seq, f.what);
    }
    say!();
    say!("The log is the only thing that cannot be recomputed. Restore it from a");
    say!("backup rather than rebuilding: `capture rebuild` recomputes the projection");
    say!("from these same events and would carry the damage across.");
}

fn describe(r: Rejection) -> String {
    match r {
        Rejection::IdMismatch => "the id does not cover the header it claims to".into(),
        Rejection::BadSignature => "the signature does not verify under its origin's key".into(),
        Rejection::BodyMismatch => "the body does not match the hash its header commits to".into(),
        Rejection::UnknownOrigin => "its origin's key is not on the roster".into(),
    }
}

fn short(d: &Dev) -> String {
    hex(&d.short())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use capture_store::ExitCode;

    #[test]
    fn integrity_has_its_own_exit_code() {
        // A damaged archive must never be reported the same way as a mistyped
        // command; a script cannot otherwise tell "retry this" from "stop".
        let e = Error::Integrity("x".into());
        assert_eq!(e.exit_code(), ExitCode::Integrity);
        assert_ne!(e.exit_code(), Error::Usage("x".into()).exit_code());
        assert_ne!(e.exit_code(), Error::NotFound("x".into()).exit_code());
    }

    #[test]
    fn every_rejection_has_its_own_words() {
        // Collapsing these into one message would leave a reader unable to tell
        // a forged signature from a payload that no longer matches its hash.
        let all = [
            Rejection::IdMismatch,
            Rejection::BadSignature,
            Rejection::BodyMismatch,
            Rejection::UnknownOrigin,
        ];
        let mut seen: Vec<String> = all.iter().map(|r| describe(*r)).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), all.len());
    }
}
