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

use capture_core::{EventId, envelope::Envelope, event::Kind, fold};

use crate::{Result, projection::Store};

/// Fold new events into an existing projection.
///
/// `all` is every event in the log; `arriving` is what has just been added.
/// The full set is needed because re-folding a note requires its whole history,
/// not only the part that just turned up.
pub fn apply(
    store: &mut Store,
    log: &crate::log::Log,
    arriving: &[Envelope],
    now: i64,
) -> Result<usize> {
    // Some events do not name the notes they change. A mute by tag or by date
    // is a *rule*, and which notes it covers is decided by matching, not by a
    // target field — so there is nothing in the event to re-fold from.
    //
    // Those go the long way round. It is the expensive path and it is the only
    // correct one: the alternative is an event that appears to have been
    // applied while the notes it governs are untouched, which is invisible
    // until somebody notices a muted tag still turning up in searches.
    if arriving.iter().any(is_global) {
        let whole = fold::fold(&log.all()?);
        store.write(&whole, now)?;
        return Ok(whole.notes.len());
    }

    let touched = affected(arriving);
    if touched.is_empty() {
        return Ok(0);
    }

    // Every event belonging to a touched note, however old — plus every rule
    // that governs notes without naming them.
    //
    // The rules have to come along. A note that has just been given a tag may
    // now fall under a mute written weeks ago, and a partial fold that left the
    // rule out would produce a note the mute plainly covers and that is not
    // muted. There are few of them and they are cheap to carry.
    //
    // Asked of the log rather than filtered out of every event it holds. This
    // read the whole log and discarded nearly all of it, on the path a write
    // waits on — two milliseconds at a hundred and fifty events, and growing
    // with the archive for ever.
    let relevant = log.events_touching(&touched)?;

    // Links resolve against every note that existed when they were written, and
    // `relevant` holds only the touched notes' events. Without the names the
    // projection already has, every link in a re-folded note would resolve to
    // nothing — and the note would render perfectly in every other respect, so
    // nothing would look wrong.
    // Only the names these events could ask about, rather than every name in
    // the archive.
    //
    // `known_names` reads every slug and every alias — two rows a note — and it
    // ran on every write. At eight thousand notes that is sixteen thousand rows
    // to build a map from, to answer at most a handful of questions.
    //
    // The fold consults the seed in exactly two places: resolving a `[[link]]`
    // written in one of these events, and refusing an alias that another note
    // already holds. The first needs the names those texts mention; the second
    // needs the name being assigned. Nothing else looks anything up, so
    // anything else in the map was carried for nothing.
    //
    // The alias case is the one with teeth: seed it too narrowly and a name
    // another note holds would look free, and "one name always means one note"
    // is the promise §4.3 makes links out of. It is a test.
    let known = store.names_matching(&referenced(&relevant))?;
    let partial = fold::fold_seeded(&relevant, &known);
    store.merge(&partial, now)?;
    Ok(touched.len())
}

/// Every name these events could ask the projection about.
///
/// The links their texts mention, and the aliases they assign. Those are the
/// only two questions the fold puts to its seed.
fn referenced(events: &[Envelope]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for e in events {
        let Some(body) = e.body.as_ref() else {
            continue;
        };
        match e.header.kind {
            Kind::CAPTURE => {
                if let Ok(b) = minicbor::decode::<capture_core::body::CaptureBody>(body) {
                    out.extend(capture_core::link::extract(&b.text));
                }
            }
            Kind::EDIT => {
                if let Ok(b) = minicbor::decode::<capture_core::body::EditBody>(body) {
                    out.extend(capture_core::link::extract(&b.text));
                }
            }
            // Both the name being taken and, so the fold can see it is this
            // note's own, the one being given up.
            Kind::ALIAS => {
                if let Ok(b) = minicbor::decode::<capture_core::body::AliasBody>(body)
                    && let Some(a) = b.alias
                {
                    out.insert(a);
                }
            }
            _ => {}
        }
    }
    out
}

/// Whether an event changes notes it does not name.
///
/// A mute carrying a note's identity in `tgt` is ordinary. One that does not is
/// a rule over a set — every note with a tag, every note from a fortnight — and
/// the set is not knowable from the event.
fn is_global(e: &Envelope) -> bool {
    matches!(e.header.kind, Kind::MUTE if e.header.tgt.is_none())
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
    use capture_core::testing;
    use std::path::{Path, PathBuf};

    fn base(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("capture-incr-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("state.db")
    }

    /// A real log holding these events, because `apply` now asks one for what
    /// it needs rather than being handed everything.
    ///
    /// Feeding it a slice was simpler and tested less: the queries behind
    /// `events_touching` are part of the rule now, and a test that bypassed
    /// them would agree with a full rebuild while the running code did not.
    fn logged(near: &Path, events: &[Envelope]) -> crate::log::Log {
        // Its own file every time. Sharing one across calls re-appends events
        // that are already in it, and the log says so — "local append out of
        // sequence" — which is the integrity check working rather than a
        // problem to route around.
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = near.with_extension(format!("events-{n}"));

        let mut log = crate::log::Log::open(&path).unwrap();
        for e in events {
            log.append(e).unwrap();
        }
        log
    }

    fn clean(p: &Path) {
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    /// Links as a comparable string, so the equivalence check covers them.
    fn links_of(s: &Store, t: &EventId) -> String {
        s.links_from(t)
            .unwrap()
            .iter()
            .map(|l| format!("{}->{}", l.name, l.resolved.is_some()))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Everything observable about a projection, for comparison.
    ///
    /// Links are included because they are the part an incremental fold is most
    /// likely to get wrong: they depend on notes *other* than the one being
    /// re-folded, so a partial fold that looks complete can silently resolve
    /// them all to nothing. That is exactly what happened, and this snapshot
    /// missing them is why running the command found it rather than the test.
    ///
    /// Divergence is included deliberately. It is the part most likely to
    /// differ between the two paths, because whether a branch is still a tip
    /// depends on events that may arrive long after the branch did — exactly
    /// the case a partial fold could get wrong while the text still looks
    /// right.
    type Snap = (Option<String>, Option<String>, u64, String, Option<String>);

    fn snapshot(s: &Store, notes: &[EventId]) -> Vec<Snap> {
        notes
            .iter()
            .map(|t| {
                (
                    s.slug_of(t).unwrap(),
                    s.text_of(t).unwrap(),
                    s.divergence_count(t).unwrap(),
                    links_of(s, t),
                    // The promotion, as a string that changes when anything
                    // about it does. Without this the equivalence was silent
                    // about the one rule in the fold that is not
                    // last-write-wins — an incremental fold could resolve a
                    // pinned promotion differently from a full one and nothing
                    // would notice.
                    promotion_of(s, t),
                )
            })
            .collect()
    }

    fn promotion_of(s: &Store, t: &EventId) -> Option<String> {
        s.promotion_of(t).unwrap().map(|p| {
            format!(
                "{}|{}|{}|{:?}|{:?}",
                p.kind, p.descriptor, p.pinned, p.grammar, p.confidence
            )
        })
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
            full.write(&fold::fold(&s.events), 1_800_000_000_000)
                .unwrap();

            // Incremental: the same events, in batches, applied one at a time.
            let incr_path = generation_path(&b, 2);
            let mut incr = Store::open(&incr_path).unwrap();
            let mut log = logged(&b.with_extension("events"), &[]);
            for chunk in s.events.chunks(3) {
                for e in chunk {
                    log.append(e).unwrap();
                }
                apply(&mut incr, &log, chunk, 1_800_000_000_000).unwrap();
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
    fn incremental_matches_a_full_rebuild_for_the_typed_kinds() {
        // Now that the projection stores promotions, this compares them. The
        // scenario deliberately puts a machine promotion and a human one on
        // every note, so an incremental fold that saw the correction in an
        // earlier batch has to still refuse the machine promotion arriving
        // later — the authority-before-recency rule, checked across two
        // independent implementations of the same fold.
        for seed in [2u64, 13, 77] {
            let b = base(&format!("equiv-typed-{seed}"));
            let s = testing::typed(seed);

            let full_path = generation_path(&b, 1);
            let mut full = Store::open(&full_path).unwrap();
            full.write(&fold::fold(&s.events), 1_800_000_000_000)
                .unwrap();

            let incr_path = generation_path(&b, 2);
            let mut incr = Store::open(&incr_path).unwrap();
            let mut log = logged(&b.with_extension("events"), &[]);
            for chunk in s.events.chunks(2) {
                for e in chunk {
                    log.append(e).unwrap();
                }
                apply(&mut incr, &log, chunk, 1_800_000_000_000).unwrap();
            }

            let snaps = snapshot(&full, &s.notes);

            // Non-vacuous, checked rather than assumed. A comparison of two
            // empty things passes for the wrong reason, and this test existed
            // in exactly that state until the projection learned to store a
            // promotion.
            assert!(
                snaps.iter().any(|s| s.4.is_some()),
                "seed {seed}: the snapshot contains no promotion, so it compares nothing"
            );
            assert!(
                snaps
                    .iter()
                    .any(|s| s.4.as_deref().is_some_and(|p| p.contains("|true|"))),
                "seed {seed}: no pinned promotion survived, so authority is untested"
            );

            assert_eq!(
                snaps,
                snapshot(&incr, &s.notes),
                "seed {seed}: incremental fold disagreed with a full rebuild"
            );
            clean(&b);
        }
    }

    #[test]
    fn a_link_survives_an_incremental_fold() {
        // The case the generated scenarios do not reach, because none of them
        // write links. An incremental fold sees only the touched note's events,
        // so without the names the projection already holds this resolves to
        // nothing — and the note renders correctly in every other respect,
        // which is what makes the failure invisible.
        let b = base("linked");
        let path = generation_path(&b, 1);
        let mut store = Store::open(&path).unwrap();

        let mut d = testing::Origin::new(0);
        let first = d.author(
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "budget planning for next quarter"),
            1_700_000_000_000,
        );
        let first_id = first.header.id();
        let mut log = logged(&b.with_extension("events"), std::slice::from_ref(&first));
        apply(
            &mut store,
            &log,
            std::slice::from_ref(&first),
            1_800_000_000_000,
        )
        .unwrap();
        let slug = store.slug_of(&first_id).unwrap().unwrap();

        // A second capture, folded in its own batch — the ordinary case, and
        // the one where the first note is not in the batch at all.
        let second = d.author(
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(2, &format!("see [[{slug}]] for figures")),
            1_700_000_001_000,
        );
        let second_id = second.header.id();
        let all = vec![first, second.clone()];
        log.append(&second).unwrap();
        apply(
            &mut store,
            &log,
            std::slice::from_ref(&second),
            1_800_000_000_000,
        )
        .unwrap();

        let links = store.links_from(&second_id).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(
            links[0].resolved,
            Some(first_id),
            "an incrementally folded link must resolve to the same note a full \
             fold gives it"
        );

        // And it agrees with a full rebuild, which is the property that matters.
        let full_path = generation_path(&b, 2);
        let mut full = Store::open(&full_path).unwrap();
        full.write(&fold::fold(&all), 1_800_000_000_000).unwrap();
        assert_eq!(links_of(&store, &second_id), links_of(&full, &second_id));
        clean(&b);
    }

    #[test]
    fn a_link_written_before_its_target_stays_unresolved_incrementally() {
        // The other direction, and the one a naive seed gets wrong: seeding an
        // incremental fold with *every* name the projection knows lets a link
        // resolve to a note created after it was written. The name has to be
        // the real slug for this to test anything — an invented one matches
        // nothing and the test passes for the wrong reason.
        let b = base("forward");
        let path = generation_path(&b, 1);
        let mut store = Store::open(&path).unwrap();

        // Authored later in time, so its name exists only from then on.
        let mut late_author = testing::Origin::new(0);
        let later = late_author.author(
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(2, "budget planning for next quarter"),
            1_700_000_900_000,
        );
        let later_id = later.header.id();
        let slug = fold::fold(std::slice::from_ref(&later)).notes[&later_id]
            .slug
            .clone();

        // A different device, writing earlier, naming that slug before the note
        // it names came into being.
        let mut early_author = testing::Origin::new(1);
        let early = early_author.author(
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, &format!("see [[{slug}]] soon")),
            1_700_000_100_000,
        );
        let early_id = early.header.id();

        let all = vec![early.clone(), later.clone()];
        let log = logged(&b.with_extension("events"), &all);
        apply(
            &mut store,
            &log,
            std::slice::from_ref(&later),
            1_800_000_000_000,
        )
        .unwrap();
        // Now re-fold the earlier note with the later name already known.
        apply(
            &mut store,
            &log,
            std::slice::from_ref(&early),
            1_800_000_000_000,
        )
        .unwrap();

        let links = store.links_from(&early_id).unwrap();
        assert_eq!(links.len(), 1, "the link is recorded either way");
        assert_eq!(
            links[0].resolved, None,
            "a link must never acquire a target its author could not have seen"
        );

        // And a full rebuild says the same thing.
        let full_path = generation_path(&b, 2);
        let mut full = Store::open(&full_path).unwrap();
        full.write(&fold::fold(&all), 1_800_000_000_000).unwrap();
        assert_eq!(links_of(&store, &early_id), links_of(&full, &early_id));
        clean(&b);
    }

    #[test]
    fn an_alias_survives_a_later_change_to_the_same_note() {
        // An incremental fold is seeded with the names the projection already
        // holds, including this note's own alias. Treating mere presence as
        // "taken" makes the note collide with itself, so the alias is silently
        // dropped the next time anything else about the note changes — and the
        // note still renders perfectly, minus its name.
        let b = base("self-collision");
        let path = generation_path(&b, 1);
        let mut store = Store::open(&path).unwrap();

        let mut d = testing::Origin::new(0);
        let cap = d.author(
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "budget planning for next quarter"),
            1_700_000_000_000,
        );
        let tgt = cap.header.id();
        let named = d.author(
            capture_core::Kind::ALIAS,
            Some(tgt),
            None,
            testing::alias_body(2, Some("q4-budget")),
            1_700_000_001_000,
        );
        let tagged = d.author(
            capture_core::Kind::TAG,
            Some(tgt),
            None,
            testing::tag_body(3, "finance"),
            1_700_000_002_000,
        );

        let mut all = vec![cap.clone()];
        apply(
            &mut store,
            &logged(&b.with_extension("events"), &all),
            std::slice::from_ref(&cap),
            1_800_000_000_000,
        )
        .unwrap();

        all.push(named.clone());
        apply(
            &mut store,
            &logged(&b.with_extension("events"), &all),
            std::slice::from_ref(&named),
            1_800_000_000_000,
        )
        .unwrap();
        assert_eq!(
            store.resolve("q4-budget").unwrap(),
            Some(tgt),
            "the alias is assigned"
        );

        // Anything at all touching the note re-folds it.
        all.push(tagged.clone());
        apply(
            &mut store,
            &logged(&b.with_extension("events"), &all),
            std::slice::from_ref(&tagged),
            1_800_000_000_000,
        )
        .unwrap();
        assert_eq!(
            store.resolve("q4-budget").unwrap(),
            Some(tgt),
            "and it is still assigned after an unrelated change"
        );

        // And it matches a full rebuild.
        let full_path = generation_path(&b, 2);
        let mut full = Store::open(&full_path).unwrap();
        full.write(&fold::fold(&all), 1_800_000_000_000).unwrap();
        assert_eq!(full.resolve("q4-budget").unwrap(), Some(tgt));
        clean(&b);
    }

    /// A mute over every note carrying a tag.
    fn tag_mute(d: &mut testing::Origin, tag: &str, at: u64) -> Envelope {
        d.author(
            capture_core::Kind::MUTE,
            None,
            None,
            testing::encode(&capture_core::body::MuteBody {
                nonce: [9u8; 16],
                scope: capture_core::body::MuteScope::Tag(tag.to_string()),
                until: None,
            }),
            at,
        )
    }

    #[test]
    fn a_rule_reaches_notes_it_does_not_name() {
        // A mute by tag names no note at all, so there is nothing in it to
        // re-fold from. Applying it as an ordinary targeted event would leave
        // every note it governs untouched — and nothing would look wrong until
        // somebody noticed a muted tag still turning up in searches.
        let b = base("rule-reaches");
        let path = generation_path(&b, 1);
        let mut store = Store::open(&path).unwrap();

        let mut d = testing::Origin::new(0);
        let cap = d.author(
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "a note that will be quietened"),
            1_700_000_000_000,
        );
        let tgt = cap.header.id();
        let tag = d.author(
            capture_core::Kind::TAG,
            Some(tgt),
            None,
            testing::tag_body(2, "noise"),
            1_700_000_001_000,
        );

        let mut all = vec![cap.clone(), tag.clone()];
        apply(
            &mut store,
            &logged(&b.with_extension("events"), &all),
            &all.clone(),
            1_800_000_000_000,
        )
        .unwrap();
        assert!(store.note(&tgt).unwrap().unwrap().muted_until.is_none());

        let mute = tag_mute(&mut d, "noise", 1_700_000_002_000);
        all.push(mute.clone());
        apply(
            &mut store,
            &logged(&b.with_extension("events"), &all),
            std::slice::from_ref(&mute),
            1_800_000_000_000,
        )
        .unwrap();

        assert_eq!(
            store.note(&tgt).unwrap().unwrap().muted_until,
            Some(0),
            "the rule must reach a note it never named"
        );
    }

    #[test]
    fn a_note_that_becomes_eligible_later_is_covered() {
        // The same problem from the other side. The note is re-folded because
        // it was tagged, and the rule that now covers it is not among its own
        // events — so a partial fold that carried only those would produce a
        // note the mute plainly covers and that is not muted.
        let b = base("becomes-eligible");
        let path = generation_path(&b, 1);
        let mut store = Store::open(&path).unwrap();

        let mut d = testing::Origin::new(0);
        let cap = d.author(
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "a note tagged after the rule"),
            1_700_000_000_000,
        );
        let tgt = cap.header.id();
        let mute = tag_mute(&mut d, "noise", 1_700_000_001_000);

        let mut all = vec![cap.clone(), mute.clone()];
        apply(
            &mut store,
            &logged(&b.with_extension("events"), &all),
            &all.clone(),
            1_800_000_000_000,
        )
        .unwrap();
        assert!(store.note(&tgt).unwrap().unwrap().muted_until.is_none());

        // Now it qualifies.
        let tag = d.author(
            capture_core::Kind::TAG,
            Some(tgt),
            None,
            testing::tag_body(3, "noise"),
            1_700_000_002_000,
        );
        all.push(tag.clone());
        apply(
            &mut store,
            &logged(&b.with_extension("events"), &all),
            std::slice::from_ref(&tag),
            1_800_000_000_000,
        )
        .unwrap();

        assert_eq!(
            store.note(&tgt).unwrap().unwrap().muted_until,
            Some(0),
            "an existing rule must cover a note that has just become eligible"
        );

        // And a full rebuild agrees, which is the property that matters.
        let full_path = generation_path(&b, 2);
        let mut full = Store::open(&full_path).unwrap();
        full.write(&fold::fold(&all), 1_800_000_000_000).unwrap();
        assert_eq!(
            full.note(&tgt).unwrap().unwrap().muted_until,
            store.note(&tgt).unwrap().unwrap().muted_until
        );
        clean(&b);
    }

    #[test]
    fn arrival_order_does_not_change_the_result() {
        // Incremental application is where order could leak in, because the
        // batches are processed as they come.
        let b = base("order");
        let s = testing::mixed(9, 3, 3, 20);

        let a_path = generation_path(&b, 1);
        let mut a = Store::open(&a_path).unwrap();
        let mut alog = logged(&b.with_extension("events-a"), &[]);
        for chunk in s.events.chunks(4) {
            for e in chunk {
                alog.append(e).unwrap();
            }
            apply(&mut a, &alog, chunk, 1_800_000_000_000).unwrap();
        }

        let shuffled = s.shuffled(777);
        let c_path = generation_path(&b, 2);
        let mut c = Store::open(&c_path).unwrap();
        // Ingested rather than appended, because that is what an out-of-order
        // arrival is. `append` is the local path and requires the next sequence
        // number — shuffled events do not have it, and the log is right to
        // refuse them there.
        let mut clog = logged(&b.with_extension("events-c"), &[]);
        for chunk in shuffled.chunks(4) {
            for e in chunk {
                let key = s
                    .keys
                    .iter()
                    .find(|(d, _)| *d == e.header.dev)
                    .map(|(_, k)| *k)
                    .expect("the scenario names every origin it wrote with");
                clog.ingest(e, &key).unwrap();
            }
            apply(&mut c, &clog, chunk, 1_800_000_000_000).unwrap();
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
        let mut log = logged(&b.with_extension("events"), &early);
        apply(&mut store, &log, &early, 1_800_000_000_000).unwrap();
        let before = snapshot(&store, &s.notes);

        let mut all = early.clone();
        all.push(late.clone());
        log.append(&late).unwrap();
        apply(&mut store, &log, &[late], 1_800_000_000_000).unwrap();
        let after = snapshot(&store, &s.notes);

        let full_path = generation_path(&b, 2);
        let mut full = Store::open(&full_path).unwrap();
        full.write(&fold::fold(&s.events), 1_800_000_000_000)
            .unwrap();

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
        apply(
            &mut store,
            &logged(&b.with_extension("events"), &s.events),
            &s.events,
            1_800_000_000_000,
        )
        .unwrap();
        let before = snapshot(&store, &s.notes);

        assert_eq!(
            apply(
                &mut store,
                &logged(&b.with_extension("events"), &s.events),
                &[],
                1_800_000_000_000
            )
            .unwrap(),
            0
        );
        assert_eq!(snapshot(&store, &s.notes), before);
        clean(&b);
    }

    #[test]
    fn reapplying_the_same_events_is_idempotent() {
        let b = base("idem");
        let s = testing::mixed(12, 2, 3, 10);
        let path = generation_path(&b, 1);
        let mut store = Store::open(&path).unwrap();

        apply(
            &mut store,
            &logged(&b.with_extension("events"), &s.events),
            &s.events,
            1_800_000_000_000,
        )
        .unwrap();
        let once = snapshot(&store, &s.notes);
        let count = store.note_count().unwrap();

        apply(
            &mut store,
            &logged(&b.with_extension("events"), &s.events),
            &s.events,
            1_800_000_000_000,
        )
        .unwrap();
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
        apply(
            &mut store,
            &logged(&b.with_extension("events"), &s.events),
            &s.events,
            1_800_000_000_000,
        )
        .unwrap();

        // One further event against one note.
        let target = s.notes[0];
        let mut d = testing::Origin::new(7);
        let e = d.author(
            capture_core::Kind::TAG,
            Some(target),
            None,
            testing::tag_body(1, "later"),
            1_700_000_500_000,
        );
        let mut all = s.events.clone();
        all.push(e.clone());

        assert_eq!(
            apply(
                &mut store,
                &logged(&b.with_extension("events"), &all),
                &[e],
                1_800_000_000_000
            )
            .unwrap(),
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
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "aardvark original"),
            1_700_000_000_000,
        );
        let tgt = cap.header.id();
        let path = generation_path(&b, 1);
        let mut store = Store::open(&path).unwrap();
        let mut log = logged(&b.with_extension("events"), std::slice::from_ref(&cap));
        apply(
            &mut store,
            &log,
            std::slice::from_ref(&cap),
            1_800_000_000_000,
        )
        .unwrap();
        assert_eq!(
            store.search("aardvark", 1_800_000_000_000).unwrap().len(),
            1
        );

        let edit = d.author(
            capture_core::Kind::EDIT,
            Some(tgt),
            Some(tgt),
            testing::edit_body(2, "pangolin replacement"),
            1_700_000_001_000,
        );
        log.append(&edit).unwrap();
        apply(&mut store, &log, &[edit], 1_800_000_000_000).unwrap();

        assert_eq!(
            store.search("pangolin", 1_800_000_000_000).unwrap().len(),
            1
        );
        assert!(
            store
                .search("aardvark", 1_800_000_000_000)
                .unwrap()
                .is_empty(),
            "the superseded text must not remain findable"
        );
        clean(&b);
    }
}
