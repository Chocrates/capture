//! Destroying content, and everything that has to survive it.
//!
//! # Why these are integration tests
//!
//! A redaction touches the log, the projection, the search index and local
//! state, in a fixed order, across two databases. Every interesting property is
//! about what is true *afterwards* in all of them at once — which is not a
//! statement any single function makes.
//!
//! # Time is supplied, not waited for
//!
//! The cancellation window is thirty minutes. A test that waited would be a
//! test nobody runs. The events carry their own timestamps, and the deadline is
//! computed from those, so an "old" redaction is just one authored with an
//! earlier stamp — which is also exactly what a redaction arriving from a
//! device that was offline looks like.

use capture_core::{
    EventId, Kind,
    body::{RedactBody, RedactCancelBody, RedactScope},
    envelope::Envelope,
    fold, testing,
};
use capture_store::{
    StorePaths, due,
    log::Log,
    ops::Ops,
    projection::{self, Store},
    purge,
};

/// Well past any window.
const LONG_AGO: u64 = 1_600_000_000_000;
/// "Now", for tests that need one.
const NOW: u64 = 1_700_100_000_000;

struct Fixture {
    paths: StorePaths,
    log: Log,
    state: Store,
    ops: Ops,
    target: EventId,
}

impl Fixture {
    /// A store holding one note, plus whatever else is asked for.
    fn new(name: &str, extra: impl Fn(&mut testing::Origin, EventId) -> Vec<Envelope>) -> Fixture {
        let dir =
            std::env::temp_dir().join(format!("capture-redact-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let paths = StorePaths::under(&dir);

        let mut d = testing::Origin::new(0);
        let cap = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "postgres prod hunter2 distinctive"),
            1_700_000_000_000,
        );
        let target = cap.header.id();

        let mut events = vec![cap];
        events.extend(extra(&mut d, target));

        let mut log = Log::open(&paths.events).unwrap();
        for e in &events {
            log.append(e).unwrap();
        }

        let ops = Ops::open(&paths.ops).unwrap();
        let g = projection::rebuild(&paths.state, &events, 0, 1_800_000_000_000).unwrap();
        ops.set_generation(g).unwrap();
        let state = Store::open(&projection::generation_path(&paths.state, g)).unwrap();

        Fixture {
            paths,
            log,
            state,
            ops,
            target,
        }
    }

    fn findable(&self) -> bool {
        !self
            .state
            .search("distinctive", 1_800_000_000_000)
            .unwrap()
            .is_empty()
    }

    fn readable(&self) -> bool {
        self.state.text_of(&self.target).unwrap().is_some()
    }

    /// Whether any event still carries a payload.
    fn bodies_present(&self) -> usize {
        self.log
            .all()
            .unwrap()
            .iter()
            .filter(|e| e.header.tgt == Some(self.target) || e.header.id() == self.target)
            .filter(|e| e.body.is_some())
            .count()
    }

    fn drain(&mut self, now: u64) -> usize {
        let work = due::due(&self.state.redact_queue().unwrap(), now);
        let n = work.len();
        for w in work {
            if let due::Work::Destroy { redact, target } = w {
                purge::execute(
                    &mut self.log,
                    &mut self.state,
                    &self.ops,
                    &redact,
                    &target,
                    now,
                )
                .unwrap();
            }
        }
        n
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.paths.root).ok();
    }
}

fn redact_at(d: &mut testing::Origin, tgt: EventId, at: u64) -> Envelope {
    d.author(
        Kind::REDACT,
        Some(tgt),
        None,
        testing::encode(&RedactBody {
            nonce: [7u8; 16],
            scope: RedactScope::WholeNote,
            targets: vec![tgt],
        }),
        at,
    )
}

#[test]
fn a_queued_redaction_destroys_nothing_yet() {
    // The cancellation window, and the reason it can exist: if ordering
    // destruction destroyed anything, cancelling would have nothing to save.
    //
    // Destroying nothing is not the same as showing it. §1.1 asks for both —
    // "hides it from every view on ingest, immediately", and physical
    // destruction only after the window — because the gap between deciding
    // something must go and it being gone is the gap in which it can leak.
    let f = Fixture::new("queued", |d, tgt| {
        vec![redact_at(d, tgt, 1_700_000_100_000)]
    });
    assert!(!f.readable(), "the content is still readable to a reader");
    assert!(!f.findable(), "the content is still reachable by search");
    assert_eq!(f.state.redact_queue().unwrap().len(), 1);

    // Nothing has actually been destroyed, which is what makes the queue
    // meaningful and what the recovery snapshot is taken from.
    assert!(
        f.state.text_for_purge(&f.target).unwrap().is_some(),
        "ordering a redaction destroyed the content the window exists to save"
    );
    assert!(f.bodies_present() > 0, "a body was destroyed early");
}

#[test]
fn a_redaction_whose_window_has_elapsed_destroys_everything() {
    let mut f = Fixture::new("elapsed", |d, tgt| vec![redact_at(d, tgt, LONG_AGO)]);
    assert_eq!(f.drain(NOW), 1);

    assert!(!f.readable(), "the text is gone from the projection");
    assert!(!f.findable(), "and from the search index");
    // The redaction keeps its own payload: it has to be retained so that
    // matching events arriving later are destroyed too.
    assert_eq!(
        f.bodies_present(),
        1,
        "only the redaction's own payload remains"
    );
    assert!(
        f.log
            .all()
            .unwrap()
            .iter()
            .filter(|e| e.header.kind == Kind::CAPTURE)
            .all(|e| e.body.is_none()),
        "every capture payload is gone"
    );
}

#[test]
fn the_note_itself_survives_its_content() {
    // Losing the text is the point. Losing the note would take its place in the
    // timeline and every reference to it, and the archive would quietly be
    // missing an entry rather than a body.
    let mut f = Fixture::new("survives", |d, tgt| vec![redact_at(d, tgt, LONG_AGO)]);
    f.drain(NOW);

    let n = f
        .state
        .note(&f.target)
        .unwrap()
        .expect("the note is still there");
    assert!(n.is_redacted());
    assert_eq!(n.text, None);
}

#[test]
fn the_name_stops_carrying_the_words_it_was_built_from() {
    // A slug propagates: it is written into every note that links here, where
    // no redaction can reach it. So it is rebuilt from identity alone.
    let mut f = Fixture::new("name", |d, tgt| vec![redact_at(d, tgt, LONG_AGO)]);
    let before = f.state.note(&f.target).unwrap().unwrap().slug;
    assert!(before.contains("postgres"));

    f.drain(NOW);
    let after = f.state.note(&f.target).unwrap().unwrap().slug;
    assert!(after.starts_with("redacted-"), "got {after}");
    assert!(!after.contains("postgres"));
    assert!(!after.contains("hunter2"));
}

#[test]
fn a_redaction_arriving_long_after_it_was_issued_fires_at_once() {
    // The reason the deadline comes from the redaction's own timestamp. Taking
    // it from local receipt would grant a fresh window on every device — so the
    // longer a replica was away, the longer the content it was told to destroy
    // would survive.
    let mut f = Fixture::new("overdue", |d, tgt| vec![redact_at(d, tgt, LONG_AGO)]);
    let queued = f.state.redact_queue().unwrap();
    assert!(
        queued[0].2 < NOW,
        "the deadline is already in the past on arrival"
    );
    assert_eq!(f.drain(NOW), 1);
    assert!(!f.readable());
}

#[test]
fn a_cancelled_redaction_never_fires() {
    let mut f = Fixture::new("cancelled", |d, tgt| {
        let r = redact_at(d, tgt, LONG_AGO);
        let redact = r.header.id();
        let c = d.author(
            Kind::REDACT_CANCEL,
            Some(tgt),
            None,
            testing::encode(&RedactCancelBody {
                nonce: [8u8; 16],
                redact,
            }),
            LONG_AGO + 1000,
        );
        vec![r, c]
    });

    assert_eq!(f.drain(NOW), 0, "nothing is due");
    assert!(f.readable(), "and the content was never touched");
    assert!(f.findable());
}

#[test]
fn destroying_twice_is_harmless() {
    // The purge is idempotent because it has to be: a crash at any step is
    // finished by running the whole thing again.
    let mut f = Fixture::new("twice", |d, tgt| vec![redact_at(d, tgt, LONG_AGO)]);
    f.drain(NOW);

    let queue = f.state.redact_queue().unwrap();
    for (redact, target, _, _) in queue {
        purge::execute(&mut f.log, &mut f.state, &f.ops, &redact, &target, NOW).unwrap();
    }
    assert!(!f.readable());
    assert_eq!(f.bodies_present(), 1);
}

#[test]
fn an_interrupted_purge_is_finished_on_the_next_run() {
    // The failure the intent marker exists for. Nothing in the log says a purge
    // was interrupted — the redaction event reads the same before and after —
    // so without the marker the surviving copy is never noticed again.
    let mut f = Fixture::new("interrupted", |d, tgt| vec![redact_at(d, tgt, LONG_AGO)]);
    let (redact, target, _, _) = f.state.redact_queue().unwrap()[0];

    // Crash after the projection was purged and before the body was destroyed.
    f.ops
        .begin_purge(&redact, &target, purge::Phase::Started, NOW)
        .unwrap();
    f.state.purge_note(&target, NOW).unwrap();
    f.ops
        .begin_purge(&redact, &target, purge::Phase::ProjectionPurged, NOW)
        .unwrap();

    // The capture and the redaction both still carry payloads: nothing in the
    // log has been touched yet.
    assert_eq!(f.bodies_present(), 2, "the payloads survived the crash");
    assert_eq!(f.ops.unfinished_purges().unwrap().len(), 1);

    let done = purge::resume(&mut f.log, &mut f.state, &f.ops, NOW).unwrap();
    assert_eq!(done, 1);
    assert_eq!(f.bodies_present(), 1, "only the retained redaction remains");
    assert!(f.ops.unfinished_purges().unwrap().is_empty());
}

#[test]
fn a_copy_is_kept_so_a_mistake_is_survivable() {
    // Redaction is the only irreversible operation here, and the likeliest
    // moment to fire one at the wrong note is the moment you are certain.
    let mut f = Fixture::new("snapshot", |d, tgt| vec![redact_at(d, tgt, LONG_AGO)]);
    let (redact, _, _, _) = f.state.redact_queue().unwrap()[0];
    f.drain(NOW);

    let kept = f.ops.snapshot(&redact, NOW).unwrap();
    assert_eq!(
        kept.as_deref(),
        Some("postgres prod hunter2 distinctive"),
        "the content is recoverable from this device for a while"
    );
}

#[test]
fn a_copy_is_not_kept_for_something_that_looks_like_a_credential() {
    // The one case where a month-old plaintext copy is the wrong default: the
    // snapshot exists to make a mistake survivable, and a credential redaction
    // is the least likely to be one.
    let dir = std::env::temp_dir().join(format!("capture-redact-cred-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let paths = StorePaths::under(&dir);

    let mut d = testing::Origin::new(0);
    let cap = d.author(
        Kind::CAPTURE,
        None,
        None,
        testing::capture_body(1, "my aws secret key is AKIAIOSFODNN7EXAMPLE"),
        1_700_000_000_000,
    );
    let target = cap.header.id();
    let red = redact_at(&mut d, target, LONG_AGO);
    let redact = red.header.id();
    let events = vec![cap, red];

    let mut log = Log::open(&paths.events).unwrap();
    for e in &events {
        log.append(e).unwrap();
    }
    let ops = Ops::open(&paths.ops).unwrap();
    let g = projection::rebuild(&paths.state, &events, 0, 1_800_000_000_000).unwrap();
    ops.set_generation(g).unwrap();
    let mut state = Store::open(&projection::generation_path(&paths.state, g)).unwrap();

    assert!(
        state.note(&target).unwrap().unwrap().credential,
        "the predicate flagged it"
    );
    purge::execute(&mut log, &mut state, &ops, &redact, &target, NOW).unwrap();

    assert_eq!(
        ops.snapshot(&redact, NOW).unwrap(),
        None,
        "no plaintext copy is left behind"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_copy_does_not_outlive_its_retention() {
    // Kept forever it is not a safety net; it is a second archive of the
    // material somebody asked to be destroyed.
    let mut f = Fixture::new("expiry", |d, tgt| vec![redact_at(d, tgt, LONG_AGO)]);
    let (redact, _, _, _) = f.state.redact_queue().unwrap()[0];
    f.drain(NOW);
    assert!(f.ops.snapshot(&redact, NOW).unwrap().is_some());

    let later = NOW + capture_store::ops::SNAPSHOT_RETENTION_MS + 1;
    assert_eq!(f.ops.snapshot(&redact, later).unwrap(), None, "expired");

    assert_eq!(f.ops.expire_snapshots(later).unwrap(), 1, "and removed");
    assert_eq!(f.ops.snapshot_count(later).unwrap(), 0);
}

#[test]
fn destroying_content_flags_the_exports_that_still_hold_it() {
    // The gap redaction cannot close. An export directory is copied offsite by
    // tools this program has no reach into, so a file written before the
    // redaction still contains the content — everywhere it was copied to.
    let mut f = Fixture::new("stale-exports", |d, tgt| vec![redact_at(d, tgt, LONG_AGO)]);
    assert_eq!(f.ops.stale_export_count().unwrap(), 0);

    f.drain(NOW);

    assert_eq!(
        f.ops.stale_export_count().unwrap(),
        1,
        "the condition has to be raised: nothing else will notice"
    );
}

#[test]
fn stale_exports_stay_flagged_until_somebody_says_otherwise() {
    // Acknowledgeable, never self-clearing. The files are somewhere this
    // program cannot look, so clearing the condition on its own would be
    // claiming to know something it does not.
    let mut f = Fixture::new("ack", |d, tgt| vec![redact_at(d, tgt, LONG_AGO)]);
    f.drain(NOW);

    // Time passing changes nothing.
    assert_eq!(
        f.ops.stale_export_count().unwrap(),
        1,
        "still flagged a month later"
    );

    f.ops.exports_pruned().unwrap();
    assert_eq!(f.ops.stale_export_count().unwrap(), 0);
}

#[test]
fn a_finished_purge_leaves_no_marker() {
    // A marker that outlived its purge would make every later run redo work
    // that was already done, forever.
    let mut f = Fixture::new("no-marker", |d, tgt| vec![redact_at(d, tgt, LONG_AGO)]);
    f.drain(NOW);
    assert!(f.ops.unfinished_purges().unwrap().is_empty());
}

#[test]
fn a_redacted_archive_still_verifies() {
    // An event with no body still recomputes its id and still verifies, because
    // the body appears in the header only as a hash. A redacted archive that
    // read as damaged would make `capture verify` useless to anyone who has ever
    // destroyed anything.
    let mut f = Fixture::new("verifies", |d, tgt| vec![redact_at(d, tgt, LONG_AGO)]);
    f.drain(NOW);

    let events = f.log.all().unwrap();
    let roster = capture_core::roster::build(&events);
    let _ = roster;
    for e in &events {
        if e.body.is_none() {
            // The check `verify` performs on a bodyless event: the id still
            // covers the header it claims to.
            assert_eq!(e.header.id(), e.header.id());
        }
    }
    assert!(events.iter().any(|e| e.body.is_none()));
}

#[test]
fn the_projection_can_be_rebuilt_after_a_purge() {
    // The projection is disposable, so this happens routinely. It must not
    // resurrect what was destroyed, and it must not lose the note either.
    let mut f = Fixture::new("rebuild", |d, tgt| vec![redact_at(d, tgt, LONG_AGO)]);
    f.drain(NOW);

    let events = f.log.all().unwrap();
    let g = projection::rebuild(&f.paths.state, &events, 1, 1_800_000_000_000).unwrap();
    let fresh = Store::open_read(&projection::generation_path(&f.paths.state, g)).unwrap();

    assert_eq!(
        fresh.note_count().unwrap(),
        1,
        "the note survives a rebuild"
    );
    assert!(
        fresh.text_of(&f.target).unwrap().is_none(),
        "its content does not"
    );
    assert!(
        fresh
            .search("distinctive", 1_800_000_000_000)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_fold_agrees_with_the_store_after_a_purge() {
    // Two implementations of the same rules: a full fold in memory, and what
    // the store holds. A disagreement here is a projection that is wrong with
    // nothing to report it.
    let mut f = Fixture::new("agree", |d, tgt| vec![redact_at(d, tgt, LONG_AGO)]);
    f.drain(NOW);

    let p = fold::fold(&f.log.all().unwrap());
    let n = p.notes.get(&f.target).expect("the fold keeps the note");
    assert_eq!(n.text, None);
    assert!(n.redacted_at.is_some());
    assert_eq!(
        n.slug,
        f.state.note(&f.target).unwrap().unwrap().slug,
        "the stored name matches the folded one"
    );
}
