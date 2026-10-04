//! A redaction destroys every note it names.
//!
//! `redact_queue` was keyed by `redact_evt` alone, and the fold writes a row
//! per pending note — so every target of one redaction overwrote the last and
//! only one was ever queued. Three captures, one redaction naming all three:
//! one queue row, one note destroyed, two keeping their full text while the
//! operation reported success.
//!
//! Latent while every `RedactBody` this build constructs names exactly one
//! target, and not latent the moment anything emits the shape §1.1 specifies.

use capture_core::body::{RedactBody, RedactScope};
use capture_core::event::Kind;
use capture_store::StorePaths;
use capture_store::session::Session;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "capture-multi-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        capture_store::store::create(&StorePaths::under(p.clone()), 1_700_000_000_000).unwrap();
        Scratch(p)
    }
    fn paths(&self) -> StorePaths {
        StorePaths::under(self.0.clone())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const PAST_THE_WINDOW: u64 = 2_000_000_000_000;

fn capture(paths: &StorePaths, text: &str) -> capture_core::EventId {
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = capture_store::session::capture_body(text).unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);
    capture_store::projection::current(paths)
        .unwrap()
        .resolve(&slug)
        .unwrap()
        .expect("the note is named")
}

/// One redaction naming several notes — the shape §1.1 specifies and no
/// shipping surface yet builds.
fn redact_all(paths: &StorePaths, targets: &[capture_core::EventId]) {
    let mut s = Session::open(paths.clone()).unwrap();
    let targets = targets.to_vec();
    s.append(Kind::REDACT, None, None, |nonce| {
        let mut b = Vec::new();
        minicbor::encode(
            &RedactBody {
                nonce,
                scope: RedactScope::WholeNote,
                targets: targets.clone(),
            },
            &mut b,
        )
        .unwrap();
        b
    })
    .unwrap();
}

fn body_survives(paths: &StorePaths, tgt: &capture_core::EventId) -> bool {
    capture_store::log::Log::open_read(&paths.events)
        .unwrap()
        .by_id(tgt)
        .unwrap()
        .is_some_and(|e| e.body.is_some())
}

#[test]
fn a_redaction_naming_three_notes_destroys_three() {
    let sc = Scratch::new("three");
    let p = sc.paths();
    let a = capture(&p, "alpha hunter2 password");
    let b = capture(&p, "bravo hunter2 password");
    let c = capture(&p, "charlie hunter2 password");
    capture_store::promote::settle(&p).unwrap();

    redact_all(&p, &[a, b, c]);

    let queued = capture_store::projection::current(&p)
        .unwrap()
        .redact_queue()
        .unwrap();
    assert_eq!(queued.len(), 3, "only some targets were queued: {queued:?}");

    let mut s = Session::open(p.clone()).unwrap();
    s.drain_due_at(PAST_THE_WINDOW).unwrap();
    drop(s);

    for (name, t) in [("alpha", a), ("bravo", b), ("charlie", c)] {
        assert!(
            !body_survives(&p, &t),
            "{name} was named for destruction and kept its body"
        );
    }
}

#[test]
fn exactly_the_cap_still_applies_to_all_of_them() {
    let sc = Scratch::new("cap");
    let p = sc.paths();
    let cap = capture_core::body::REDACT_TARGET_CAP;
    let tgts: Vec<_> = (0..cap)
        .map(|i| capture(&p, &format!("note {i}")))
        .collect();
    capture_store::promote::settle(&p).unwrap();

    redact_all(&p, &tgts);

    let queued = capture_store::projection::current(&p)
        .unwrap()
        .redact_queue()
        .unwrap();
    assert_eq!(queued.len(), cap, "the cap itself must be allowed");
}

#[test]
fn more_than_the_cap_applies_to_nothing() {
    // It used to take the first 64 and drop the rest with no record, so a
    // redaction naming a hundred reported success and left thirty-six intact.
    // A redaction of the first 64 is not what anybody asked for.
    let sc = Scratch::new("overcap");
    let p = sc.paths();
    let cap = capture_core::body::REDACT_TARGET_CAP;
    let tgts: Vec<_> = (0..cap + 5)
        .map(|i| capture(&p, &format!("note {i}")))
        .collect();
    capture_store::promote::settle(&p).unwrap();

    redact_all(&p, &tgts);

    let store = capture_store::projection::current(&p).unwrap();
    assert!(
        store.redact_queue().unwrap().is_empty(),
        "an over-cap redaction queued something"
    );
    assert!(
        store.held_count().unwrap() > 0,
        "an over-cap redaction was dropped silently rather than held"
    );
    for t in &tgts {
        assert!(body_survives(&p, t), "something was destroyed anyway");
    }
}

#[test]
fn the_incremental_path_queues_every_target_too() {
    // The cheap path has to reach the same answer as a full fold or it is not a
    // cheap version of it. `affected` mapped each event to its header target,
    // and a redaction's header holds one where its body may name sixty-four —
    // so the others were never re-written and their destructions never queued.
    //
    // This is what the test above exercises by default now; stated separately
    // so that removing the incremental half fails something that says why.
    let sc = Scratch::new("incremental");
    let p = sc.paths();
    let a = capture(&p, "first");
    let b = capture(&p, "second");
    capture_store::promote::settle(&p).unwrap();

    redact_all(&p, &[a, b]);

    assert_eq!(
        capture_store::projection::current(&p)
            .unwrap()
            .redact_queue()
            .unwrap()
            .len(),
        2,
        "the incremental fold queued only the header's target"
    );
}

#[test]
fn separate_single_target_redactions_still_work() {
    // The guard: the new key must not break the ordinary case, which is every
    // redaction this build issues.
    let sc = Scratch::new("singles");
    let p = sc.paths();
    let a = capture(&p, "one");
    let b = capture(&p, "two");
    capture_store::promote::settle(&p).unwrap();

    redact_all(&p, &[a]);
    redact_all(&p, &[b]);

    assert_eq!(
        capture_store::projection::current(&p)
            .unwrap()
            .redact_queue()
            .unwrap()
            .len(),
        2
    );
}
