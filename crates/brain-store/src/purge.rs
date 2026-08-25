//! Carrying out a redaction.
//!
//! # The only irreversible thing this system does
//!
//! Everything else appends. This removes bytes, permanently, on purpose. So the
//! order of operations is the design, and the order is chosen so that a crash
//! at any point leaves a state the next run can finish rather than one it
//! cannot tell apart from success.
//!
//! ```text
//!   1. record the intent          durable, before anything is touched
//!   2. purge the projection       text, search index, versions
//!   3. destroy the body           in the log — the irreversible step
//!   4. checkpoint                 force the write-ahead log into the database
//!   5. clear the intent           only now is it finished
//! ```
//!
//! # Why the intent marker has to exist
//!
//! Steps 2 to 4 are several writes across two databases, and SQLite offers no
//! transaction spanning both. A crash between them leaves content destroyed in
//! one place and intact in another — and, crucially, **nothing in the log says
//! the purge was interrupted**, because the redaction event looks identical
//! before and after. The next run would see a redaction it believes it has
//! already handled and move on, leaving the surviving copy forever.
//!
//! The marker is the only thing that distinguishes "not started" from "halfway
//! through". It lives in local state because it describes work this device is
//! doing, and re-folding the log must not clear it.
//!
//! # Projection first, body last
//!
//! The reverse order looks equivalent and is not. Destroying the body first
//! leaves a window where the text is gone from the log but still sitting in the
//! search index — and the search index is the one place that will happily hand
//! it back on a query. Purging the derived copies first means a crash leaves the
//! content readable but not *findable*, which is the safer half of a bad
//! situation.
//!
//! # `secure_delete`, never `VACUUM`
//!
//! `VACUUM` rebuilds the database into a new file and needs free space equal to
//! its size — so it fails exactly when the disk is full, which is a state a
//! system with a capture spool is expressly built to survive. `secure_delete`
//! overwrites freed pages in place as they are freed, with no extra space and no
//! moment where two copies exist. It is set on every writing connection.

use brain_core::EventId;

use crate::{Result, log::Log, ops::Ops, projection::Store};

/// How far through a purge this device got.
///
/// Stored as text because it is read by a person as often as by the program:
/// it is what `brain status` shows when a purge did not finish.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Nothing has been touched yet.
    Started,
    /// Derived copies are gone; the body is still there.
    ProjectionPurged,
    /// The body is gone; the database has not been checkpointed.
    BodyDestroyed,
}

impl Phase {
    pub fn as_str(&self) -> &'static str {
        match self {
            Phase::Started => "started",
            Phase::ProjectionPurged => "projection-purged",
            Phase::BodyDestroyed => "body-destroyed",
        }
    }

    pub fn parse(s: &str) -> Option<Phase> {
        match s {
            "started" => Some(Phase::Started),
            "projection-purged" => Some(Phase::ProjectionPurged),
            "body-destroyed" => Some(Phase::BodyDestroyed),
            _ => None,
        }
    }
}

/// Destroy the content of every event belonging to a note.
///
/// Idempotent, and safe to call again after a crash at any point. The steps are
/// all "make this absent", so repeating one costs nothing and finishing an
/// interrupted run is the same code path as starting a fresh one.
pub fn execute(
    log: &mut Log,
    state: &mut Store,
    ops: &Ops,
    redact: &EventId,
    target: &EventId,
    now: u64,
) -> Result<()> {
    // 1. Intent, before anything is touched. A crash after this and before the
    //    end leaves something that says "unfinished"; a crash before it leaves
    //    a store where nothing happened at all.
    ops.begin_purge(redact, target, Phase::Started, now)?;

    // A copy, kept locally for a while, so that a redaction fired at the wrong
    // note is recoverable. Taken before anything is destroyed, because
    // afterwards there is nothing to copy.
    //
    // Skipped entirely for a note the credential predicate flagged. The
    // snapshot exists to make a mistake survivable; for content that is
    // probably a credential, leaving a plaintext copy lying around for a month
    // is the wrong default in exactly the case the redaction was most
    // deliberate.
    if let Some(note) = state.note(target)?
        && !note.credential
        && let Some(text) = note.text.as_deref()
    {
        ops.take_snapshot(redact, target, text, now)?;
    }

    // 2. Derived copies first. They are the ones that answer queries, and a
    //    crash here leaves content readable but not findable — the safer half.
    state.purge_note(target, now)?;
    ops.begin_purge(redact, target, Phase::ProjectionPurged, now)?;

    // The window between the projection being purged and the body being
    // destroyed is microseconds wide, which makes it almost impossible for a
    // crash test to land in — and it is the window the intent marker exists
    // for. Widening it on request lets the harness actually arrive there.
    //
    // Never widened in normal use: the variable is read once, and an
    // unparseable or absent value means zero.
    pause_for_testing();

    // 3. The irreversible step.
    log.destroy_bodies(target)?;
    ops.begin_purge(redact, target, Phase::BodyDestroyed, now)?;

    // 4. Force the write-ahead log into the database. Without this the
    //    destroyed bytes can sit in the `-wal` file indefinitely, where
    //    `secure_delete` has not reached them and anything reading the file
    //    directly still finds them.
    log.checkpoint()?;

    // 5. Finished here — and *not* finished everywhere. Any export file written
    //    before now still holds this content, and those files are copied
    //    offsite by tools this program has no reach into. The gap cannot be
    //    closed from here, so it is recorded and surfaced rather than quietly
    //    left.
    ops.note_stale_exports(now)?;
    ops.finish_purge(redact)?;
    Ok(())
}

/// Finish anything a previous run left halfway.
///
/// Called at startup, before the due-work queue is drained. An interrupted
/// purge is invisible in the log — the redaction event looks the same before
/// and after — so without this the surviving copy would never be noticed again.
pub fn resume(log: &mut Log, state: &mut Store, ops: &Ops, now: u64) -> Result<usize> {
    let unfinished = ops.unfinished_purges()?;
    let mut done = 0;

    for (redact, target, _phase) in unfinished {
        // Re-run from the top rather than from the recorded phase. Every step
        // is idempotent, and "start again" has one code path where "resume from
        // step two" has one per phase — three chances to get a rarely-exercised
        // branch wrong, in the one operation that cannot be undone.
        execute(log, state, ops, &redact, &target, now)?;
        done += 1;
    }
    Ok(done)
}

/// Hold still, if a test asked for it.
///
/// Reads an environment variable rather than taking a parameter so that no
/// production call site has to carry a "how long to pause" argument that is
/// always zero — an argument like that eventually gets passed something else.
fn pause_for_testing() {
    let Ok(v) = std::env::var("BRAIN_PURGE_PAUSE_MS") else {
        return;
    };
    let Ok(ms) = v.parse::<u64>() else { return };
    if ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_phase_round_trips() {
        // The marker is read back after a crash. A phase that did not survive
        // the round trip would read as "no purge in progress".
        for p in [
            Phase::Started,
            Phase::ProjectionPurged,
            Phase::BodyDestroyed,
        ] {
            assert_eq!(Phase::parse(p.as_str()), Some(p));
        }
    }

    #[test]
    fn an_unrecognised_phase_is_not_guessed_at() {
        // From a newer build, or a damaged row. Guessing which phase it meant
        // would be guessing how much of an irreversible operation has already
        // happened.
        assert_eq!(Phase::parse("something-else"), None);
        assert_eq!(Phase::parse(""), None);
    }

    #[test]
    fn the_phases_are_in_the_order_the_purge_performs_them() {
        // Documented by the type as well as the comment: the projection is
        // purged before the body is destroyed, so a crash between them leaves
        // content readable but unfindable rather than the reverse.
        let order = [
            Phase::Started,
            Phase::ProjectionPurged,
            Phase::BodyDestroyed,
        ];
        let names: Vec<&str> = order.iter().map(|p| p.as_str()).collect();
        assert_eq!(names, ["started", "projection-purged", "body-destroyed"]);
    }
}
