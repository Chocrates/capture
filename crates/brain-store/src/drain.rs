//! Moving spooled captures into the log.
//!
//! # The spool holds payloads, not events
//!
//! The spool exists for the case where the durable path is unavailable — the
//! disk is full, the database will not open. In that state a sequence number
//! cannot be allocated, because allocating one means reading `heads`. So what
//! is written to the spool is the capture's **body**, and the event around it is
//! authored later, when the log is back.
//!
//! That is what makes draining interesting. The event does not exist yet, so it
//! has no identity yet, so the obvious way to make the drain repeatable — check
//! whether this event is already in the log — has nothing to check.
//!
//! # Exactly once, across a crash, without a second bookkeeping store
//!
//! The drain must survive being killed at any point. Two orderings are
//! available and one of them is wrong:
//!
//! - **Clear the spool, then commit.** A crash in between loses the capture
//!   outright. Unacceptable: the interface already told the user it was saved.
//! - **Commit, then clear the spool.** A crash in between commits the capture a
//!   second time on the next attempt — unless something recognises it.
//!
//! The second ordering is the right one, so the recognition has to work. A
//! natural instinct is to record progress in local state, but the marker and
//! the event live in different databases and SQLite offers no atomic commit
//! across two, so the marker can be lost or written without its event and the
//! problem reappears one layer down.
//!
//! Instead the payload names itself. Every body opens with sixteen random
//! bytes, so a body is unique to the moment it was captured, and its hash is
//! already stored on every event. "Is this payload in the log" is therefore
//! answerable without any extra state, and answerable after a crash, and
//! answerable no matter what sequence number the earlier attempt used.
//!
//! # What a crash actually leaves behind
//!
//! Killed mid-drain, the spool still holds everything, including the records
//! already committed. The next drain re-reads all of them, skips the ones the
//! log recognises, commits the rest, and only then clears. Duplicated work,
//! never duplicated data, and never lost data.

use std::path::Path;

use brain_core::{envelope::Envelope, id::BodyHash};

use crate::{Result, log::Log, spool};

/// What a drain did.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Drained {
    /// Captures committed to the log by this call.
    pub committed: usize,
    /// Captures the log already had — evidence of an earlier interrupted drain.
    pub already_held: usize,
}

impl Drained {
    /// How many records were examined.
    pub fn seen(&self) -> usize {
        self.committed + self.already_held
    }
}

/// Commit every spooled capture, then clear the spool.
///
/// `author` turns a payload into a signed event. It is supplied rather than
/// built here because authoring needs the device key and the clock, and this
/// module has no business holding either — which also lets the crash harness
/// drive a real drain without a real identity.
pub fn drain<F>(spool_path: &Path, log: &mut Log, mut author: F) -> Result<Drained>
where
    F: FnMut(&[u8]) -> Result<Envelope>,
{
    let records = spool::read_all(spool_path)?;
    let mut out = Drained::default();

    for rec in &records {
        if log.holds_body(&BodyHash::of(&rec.payload))? {
            // Committed by an attempt that was killed before it could clear the
            // spool. Committing it again would make two notes out of one.
            out.already_held += 1;
            continue;
        }
        let e = author(&rec.payload)?;
        log.append(&e)?;
        out.committed += 1;
    }

    // Last, and only once every record above is durable. Doing this first, or
    // per record, reintroduces the window where a capture exists nowhere.
    if !records.is_empty() {
        spool::clear(spool_path)?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::testing;
    use std::path::PathBuf;

    struct Bench {
        dir: PathBuf,
        log: Log,
        spool: PathBuf,
        origin: testing::Origin,
    }

    fn bench(name: &str) -> Bench {
        let dir = std::env::temp_dir().join(format!("brain-drain-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let spool = dir.join("spool");
        spool::preallocate(&spool).unwrap();
        Bench {
            log: Log::open(&dir.join("events.db")).unwrap(),
            spool,
            origin: testing::Origin::new(0),
            dir,
        }
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    impl Bench {
        /// Author an event around a payload, the way a real drain would.
        fn author(&mut self, payload: &[u8]) -> Envelope {
            self.origin.author(
                brain_core::Kind::CAPTURE,
                None,
                None,
                payload.to_vec(),
                1_700_000_000_000,
            )
        }
    }

    fn payload(n: u8, text: &str) -> Vec<u8> {
        testing::capture_body(n, text)
    }

    #[test]
    fn an_empty_spool_drains_to_nothing() {
        let mut b = bench("empty");
        let mut o = testing::Origin::new(0);
        let d = drain(&b.spool, &mut b.log, |p| {
            Ok(o.author(
                brain_core::Kind::CAPTURE,
                None,
                None,
                p.to_vec(),
                1_700_000_000_000,
            ))
        })
        .unwrap();
        assert_eq!(d.seen(), 0);
        assert_eq!(b.log.count().unwrap(), 0);
    }

    #[test]
    fn every_spooled_capture_reaches_the_log_and_the_spool_empties() {
        let mut b = bench("basic");
        for i in 0..5u8 {
            spool::push(&b.spool, &payload(i + 1, &format!("capture {i}"))).unwrap();
        }

        let mut o = testing::Origin::new(0);
        let d = drain(&b.spool, &mut b.log, |p| {
            Ok(o.author(
                brain_core::Kind::CAPTURE,
                None,
                None,
                p.to_vec(),
                1_700_000_000_000,
            ))
        })
        .unwrap();

        assert_eq!(d.committed, 5);
        assert_eq!(b.log.count().unwrap(), 5);
        assert!(!spool::in_use(&b.spool).unwrap());
    }

    #[test]
    fn a_drain_killed_before_clearing_does_not_duplicate_anything() {
        // The exact crash the ordering is chosen for: committed, then killed,
        // so the spool still holds records the log already has.
        let mut b = bench("interrupted");
        for i in 0..4u8 {
            spool::push(&b.spool, &payload(i + 1, &format!("capture {i}"))).unwrap();
        }

        // First attempt: commit everything by hand, then die before clearing.
        let records = spool::read_all(&b.spool).unwrap();
        for r in &records[..3] {
            let e = b.author(&r.payload);
            b.log.append(&e).unwrap();
        }
        assert_eq!(b.log.count().unwrap(), 3);
        assert!(
            spool::in_use(&b.spool).unwrap(),
            "the spool was never cleared"
        );

        // Second attempt: the real drain, over the same spool.
        let mut o = testing::Origin::new(1);
        let d = drain(&b.spool, &mut b.log, |p| {
            Ok(o.author(
                brain_core::Kind::CAPTURE,
                None,
                None,
                p.to_vec(),
                1_700_000_000_000,
            ))
        })
        .unwrap();

        assert_eq!(d.already_held, 3, "the committed ones are recognised");
        assert_eq!(d.committed, 1, "and only the remainder is committed");
        assert_eq!(b.log.count().unwrap(), 4, "one note per capture, not seven");
        assert!(!spool::in_use(&b.spool).unwrap());
    }

    #[test]
    fn recognition_survives_a_different_sequence_number() {
        // Why the body's hash is the name used and the event's id is not: the
        // id covers the sequence number and timestamp, which the second attempt
        // assigns differently. An id-based check would recognise nothing.
        let mut b = bench("resequenced");
        let p = payload(1, "captured once");
        spool::push(&b.spool, &p).unwrap();

        // Commit it under one origin and sequence.
        let e = b.author(&p);
        b.log.append(&e).unwrap();
        let first_id = e.header.id();

        // Drain under a different origin entirely — a restored device, say.
        let mut other = testing::Origin::new(3);
        let d = drain(&b.spool, &mut b.log, |p| {
            Ok(other.author(
                brain_core::Kind::CAPTURE,
                None,
                None,
                p.to_vec(),
                1_700_000_000_000,
            ))
        })
        .unwrap();

        assert_eq!(d.already_held, 1);
        assert_eq!(b.log.count().unwrap(), 1);
        let held = b.log.all().unwrap();
        assert_eq!(held[0].header.id(), first_id);
    }

    #[test]
    fn draining_twice_changes_nothing_the_second_time() {
        let mut b = bench("twice");
        spool::push(&b.spool, &payload(1, "once")).unwrap();
        let mut o = testing::Origin::new(0);
        let mut go = |log: &mut Log| {
            drain(&b.spool, log, |p| {
                Ok(o.author(
                    brain_core::Kind::CAPTURE,
                    None,
                    None,
                    p.to_vec(),
                    1_700_000_000_000,
                ))
            })
            .unwrap()
        };
        assert_eq!(go(&mut b.log).committed, 1);
        assert_eq!(go(&mut b.log).seen(), 0, "the spool is empty now");
        assert_eq!(b.log.count().unwrap(), 1);
    }

    #[test]
    fn a_failure_partway_leaves_the_spool_intact() {
        // Nothing is discarded until everything is durable, so a drain that
        // cannot finish loses no captures — it merely has more to do next time.
        let mut b = bench("partial");
        for i in 0..3u8 {
            spool::push(&b.spool, &payload(i + 1, &format!("capture {i}"))).unwrap();
        }

        let mut o = testing::Origin::new(0);
        let mut n = 0;
        let err = drain(&b.spool, &mut b.log, |p| {
            n += 1;
            if n == 2 {
                return Err(crate::Error::Usage("disk went away".into()));
            }
            Ok(o.author(
                brain_core::Kind::CAPTURE,
                None,
                None,
                p.to_vec(),
                1_700_000_000_000,
            ))
        });

        assert!(err.is_err());
        assert_eq!(
            spool::read_all(&b.spool).unwrap().len(),
            3,
            "every capture is still spooled, including the one already committed"
        );
        assert_eq!(b.log.count().unwrap(), 1);
    }
}
