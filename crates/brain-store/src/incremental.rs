//! Updating the projection without rebuilding all of it.
//!
//! # Two implementations of one set of rules
//!
//! A full rebuild folds every event. An incremental update folds only what the
//! new events touch. Both must produce the same answer, and nothing about
//! writing them separately makes that true — they are two implementations of
//! the same rules, and the moment they disagree the projection is silently
//! wrong in a way no error surfaces.
//!
//! So the equivalence is a property test rather than an intention: fold
//! everything, fold in arbitrary batches, compare. It is the most valuable test
//! in this module and the reason the module is written this way.
//!
//! # How the work is bounded
//!
//! New events name the notes they touch. Those notes are re-folded from *all*
//! their events — not patched — because patching a note in place would mean a
//! second, subtly different set of rules for "how state changes" alongside the
//! fold itself. Re-folding a handful of notes is cheap; a second rule set is
//! not.

use std::collections::BTreeSet;

use brain_core::{EventId, envelope::Envelope, fold};

use crate::{Result, projection::Store};

/// Fold new events into an existing projection.
///
/// `all` is every event in the log; `arriving` is what has just been added.
/// The full set is needed because re-folding a note requires its whole history,
/// not only the part that just turned up.
pub fn apply(store: &mut Store, all: &[Envelope], arriving: &[Envelope]) -> Result<usize> {
    let touched = affected(arriving);
    if touched.is_empty() {
        return Ok(0);
    }

    // Every event belonging to a touched note, however old.
    let relevant: Vec<Envelope> = all
        .iter()
        .filter(|e| {
            e.header
                .tgt
                .map(|t| touched.contains(&t))
                .unwrap_or_else(|| touched.contains(&e.header.id()))
        })
        .cloned()
        .collect();

    let partial = fold::fold(&relevant);
    store.merge(&partial)?;
    Ok(touched.len())
}

/// Which notes a batch of events touches.
///
/// An event with no target is a capture, and a capture *is* a note — its target
/// is its own identity. It cannot say so in the event, because the identity is
/// computed over the target field, so writing it in would change the value
/// being written.
fn affected(arriving: &[Envelope]) -> BTreeSet<EventId> {
    arriving
        .iter()
        .map(|e| e.header.tgt.unwrap_or_else(|| e.header.id()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::{Store, generation_path};
    use brain_core::testing;
    use std::path::{Path, PathBuf};

    fn base(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("brain-incr-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("state.db")
    }

    fn clean(p: &Path) {
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    /// Everything observable about a projection, for comparison.
    ///
    /// Divergence is included deliberately. It is the part most likely to
    /// differ between the two paths, because whether a branch is still a tip
    /// depends on events that may arrive long after the branch did — exactly
    /// the case a partial fold could get wrong while the text still looks
    /// right.
    type Snap = (Option<String>, Option<String>, u64);

    fn snapshot(s: &Store, notes: &[EventId]) -> Vec<Snap> {
        notes
            .iter()
            .map(|t| {
                (
                    s.slug_of(t).unwrap(),
                    s.text_of(t).unwrap(),
                    s.divergence_count(t).unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn incremental_matches_a_full_rebuild() {
        // The property that keeps two implementations of one rule set honest.
        // A disagreement here is a projection that is wrong with no error.
        for seed in 1..8u64 {
            let b = base(&format!("equiv-{seed}"));
            let s = testing::mixed(seed, 3, 4, 25);

            // Full: one fold over everything.
            let full_path = generation_path(&b, 1);
            let mut full = Store::open(&full_path).unwrap();
            full.write(&fold::fold(&s.events)).unwrap();

            // Incremental: the same events, in batches, applied one at a time.
            let incr_path = generation_path(&b, 2);
            let mut incr = Store::open(&incr_path).unwrap();
            let mut seen: Vec<Envelope> = Vec::new();
            for chunk in s.events.chunks(3) {
                seen.extend(chunk.iter().cloned());
                apply(&mut incr, &seen, chunk).unwrap();
            }

            assert_eq!(
                full.note_count().unwrap(),
                incr.note_count().unwrap(),
                "seed {seed}: note count diverged"
            );
            assert_eq!(
                snapshot(&full, &s.notes),
                snapshot(&incr, &s.notes),
                "seed {seed}: incremental fold disagreed with a full rebuild"
            );
            clean(&b);
        }
    }

    #[test]
    fn arrival_order_does_not_change_the_result() {
        // Incremental application is where order could leak in, because the
        // batches are processed as they come.
        let b = base("order");
        let s = testing::mixed(9, 3, 3, 20);

        let a_path = generation_path(&b, 1);
        let mut a = Store::open(&a_path).unwrap();
        let mut seen = Vec::new();
        for chunk in s.events.chunks(4) {
            seen.extend(chunk.iter().cloned());
            apply(&mut a, &seen, chunk).unwrap();
        }

        let shuffled = s.shuffled(777);
        let c_path = generation_path(&b, 2);
        let mut c = Store::open(&c_path).unwrap();
        let mut seen2 = Vec::new();
        for chunk in shuffled.chunks(4) {
            seen2.extend(chunk.iter().cloned());
            apply(&mut c, &seen2, chunk).unwrap();
        }

        assert_eq!(snapshot(&a, &s.notes), snapshot(&c, &s.notes));
        clean(&b);
    }

    #[test]
    fn a_late_arrival_is_folded_rather_than_skipped() {
        // A single watermark would skip this: the event's timestamp is older
        // than material already folded, so a "everything up to time T" marker
        // would step straight over it.
        let b = base("late");
        let s = testing::branch_vs_single(1);
        let path = generation_path(&b, 1);
        let mut store = Store::open(&path).unwrap();

        // Hold back the edit that decides the outcome. Without it the note has
        // two live branches; with it, one side is superseded and the divergence
        // goes away — so a projection that skipped it would keep reporting a
        // conflict that no longer exists.
        let mut early = s.events.clone();
        let late = early.remove(2);
        apply(&mut store, &early, &early).unwrap();
        let before = snapshot(&store, &s.notes);

        let mut all = early.clone();
        all.push(late.clone());
        apply(&mut store, &all, &[late]).unwrap();
        let after = snapshot(&store, &s.notes);

        let full_path = generation_path(&b, 2);
        let mut full = Store::open(&full_path).unwrap();
        full.write(&fold::fold(&s.events)).unwrap();

        assert_ne!(before, after, "the late event must have changed something");
        assert_eq!(
            after,
            snapshot(&full, &s.notes),
            "a late arrival must land in the same state as folding everything at once"
        );
        clean(&b);
    }

    #[test]
    fn applying_nothing_changes_nothing() {
        let b = base("empty");
        let s = testing::mixed(11, 1, 2, 4);
        let path = generation_path(&b, 1);
        let mut store = Store::open(&path).unwrap();
        apply(&mut store, &s.events, &s.events).unwrap();
        let before = snapshot(&store, &s.notes);

        assert_eq!(apply(&mut store, &s.events, &[]).unwrap(), 0);
        assert_eq!(snapshot(&store, &s.notes), before);
        clean(&b);
    }

    #[test]
    fn reapplying_the_same_events_is_idempotent() {
        let b = base("idem");
        let s = testing::mixed(12, 2, 3, 10);
        let path = generation_path(&b, 1);
        let mut store = Store::open(&path).unwrap();

        apply(&mut store, &s.events, &s.events).unwrap();
        let once = snapshot(&store, &s.notes);
        let count = store.note_count().unwrap();

        apply(&mut store, &s.events, &s.events).unwrap();
        assert_eq!(snapshot(&store, &s.notes), once);
        assert_eq!(store.note_count().unwrap(), count, "no duplicated rows");
        clean(&b);
    }

    #[test]
    fn only_touched_notes_are_rewritten() {
        let b = base("bounded");
        let s = testing::mixed(13, 2, 4, 16);
        let path = generation_path(&b, 1);
        let mut store = Store::open(&path).unwrap();
        apply(&mut store, &s.events, &s.events).unwrap();

        // One further event against one note.
        let target = s.notes[0];
        let mut d = testing::Origin::new(7);
        let e = d.author(
            brain_core::Kind::TAG,
            Some(target),
            None,
            testing::tag_body(1, "later"),
            1_700_000_500_000,
        );
        let mut all = s.events.clone();
        all.push(e.clone());

        assert_eq!(
            apply(&mut store, &all, &[e]).unwrap(),
            1,
            "one event against one note must re-fold exactly one note"
        );
        clean(&b);
    }

    #[test]
    fn search_stays_correct_after_an_incremental_update() {
        // The index is rewritten per note, so a partial update is where stale
        // entries would survive.
        let b = base("search-incr");
        let mut d = testing::Origin::new(0);
        let cap = d.author(
            brain_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "aardvark original"),
            1_700_000_000_000,
        );
        let tgt = cap.header.id();
        let path = generation_path(&b, 1);
        let mut store = Store::open(&path).unwrap();
        apply(
            &mut store,
            std::slice::from_ref(&cap),
            std::slice::from_ref(&cap),
        )
        .unwrap();
        assert_eq!(store.search("aardvark").unwrap().len(), 1);

        let edit = d.author(
            brain_core::Kind::EDIT,
            Some(tgt),
            Some(tgt),
            testing::edit_body(2, "pangolin replacement"),
            1_700_000_001_000,
        );
        let all = vec![cap, edit.clone()];
        apply(&mut store, &all, &[edit]).unwrap();

        assert_eq!(store.search("pangolin").unwrap().len(), 1);
        assert!(
            store.search("aardvark").unwrap().is_empty(),
            "the superseded text must not remain findable"
        );
        clean(&b);
    }
}
