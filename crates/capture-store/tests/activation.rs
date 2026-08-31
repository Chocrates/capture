//! Activation: a recurring thing surfacing what was captured while it was on.
//!
//! §3.9 allows for one event kind here. None turned out to be needed, and that
//! is the interesting part: the capture already carries when it happened and
//! the series already says when it was on, so the association falls out of the
//! two. Nothing has to be filed by hand, and nothing can go stale.

// Reading the clock, because captures are stamped with it and the windows this
// tests are compared against real timestamps.
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "comparing against timestamps the store already wrote"
)]

use capture_core::body::{Promoted, RuleSpec, SeriesFields, When};
use capture_store::session::Session;
use capture_store::{StorePaths, promote};

const HOUR: i64 = 60 * 60 * 1000;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("capture-act-{name}-{}", std::process::id()));
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

fn capture(paths: &StorePaths, text: &str) -> String {
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = capture_store::session::capture_body(text).unwrap();
    s.capture(&body).unwrap().slug
}

/// A daily meeting whose most recent instance is happening right now.
fn meeting(paths: &StorePaths) -> capture_core::EventId {
    let slug = capture(paths, "standup every day");

    promote::correct(
        paths,
        &slug,
        Promoted::Series(SeriesFields {
            when: When::Timed {
                // Started an hour ago, so this instance is under way and the
                // notes taken just now fall inside it.
                start_ms: now() - HOUR / 2,
                end_ms: None,
                tz: "UTC".into(),
            },
            rule: RuleSpec {
                freq: 0,
                interval: 1,
                by_day: 0,
                by_month_day: Vec::new(),
                count: None,
                until: None,
            },
            carries_forward: false,
            split_from: None,
        }),
    )
    .unwrap();

    let s = Session::open_for_capture(paths.clone()).unwrap();
    s.state.resolve(&slug).unwrap().unwrap()
}

fn store(paths: &StorePaths) -> capture_store::projection::Store {
    let generation = capture_store::ops::Ops::open_read(&paths.ops)
        .unwrap()
        .generation()
        .unwrap();
    capture_store::projection::Store::open_read(&capture_store::projection::generation_path(
        &paths.state,
        generation,
    ))
    .unwrap()
}

#[test]
fn notes_taken_during_it_are_surfaced() {
    let sc = Scratch::new("during");
    let p = sc.paths();
    let series = meeting(&p);

    let taken = capture(&p, "sarah is blocked on the migration");

    let found = store(&p)
        .captured_during(&series, now() + HOUR, 50)
        .unwrap();
    assert!(
        found.iter().any(|n| n.slug == taken),
        "a note taken during the meeting was not surfaced: {:?}",
        found.iter().map(|n| &n.slug).collect::<Vec<_>>()
    );
}

#[test]
fn the_series_does_not_surface_itself() {
    // It was captured before its own first instance, and it is the thing being
    // asked about rather than an answer to it.
    let sc = Scratch::new("self");
    let p = sc.paths();
    let series = meeting(&p);
    capture(&p, "something said in the meeting");

    let found = store(&p)
        .captured_during(&series, now() + HOUR, 50)
        .unwrap();
    assert!(found.iter().all(|n| n.tgt != series));
}

#[test]
fn an_instance_that_has_not_happened_yet_surfaces_nothing() {
    // A meeting activates on what was said at previous ones. Sweeping in future
    // occurrences would mean the answer changed depending on how far ahead the
    // horizon had been expanded.
    let sc = Scratch::new("future");
    let p = sc.paths();
    let series = meeting(&p);
    capture(&p, "said during the current instance");

    let before_any = store(&p)
        .captured_during(&series, now() - 10 * HOUR, 50)
        .unwrap();
    assert!(
        before_any.is_empty(),
        "occurrences after the cutoff were included"
    );
}

#[test]
fn nothing_is_filed_by_hand() {
    // The association is derived, so no event records it. If one did, this
    // would need maintaining — and an association somebody has to maintain is
    // one that goes stale.
    let sc = Scratch::new("derived");
    let p = sc.paths();
    let series = meeting(&p);
    capture(&p, "a thing said at standup");

    let s = Session::open_for_capture(p.clone()).unwrap();
    let kinds: Vec<u16> = s
        .log
        .all()
        .unwrap()
        .iter()
        .map(|e| e.header.kind.0)
        .collect();
    drop(s);

    // Captures, one promotion, and nothing else. No kind exists for "this note
    // belongs to that meeting", because none is needed.
    assert!(
        kinds.iter().all(|k| *k == 1 || *k == 16),
        "an unexpected kind was written: {kinds:?}"
    );

    assert!(
        !store(&p)
            .captured_during(&series, now() + HOUR, 50)
            .unwrap()
            .is_empty(),
        "and it still finds the note"
    );
}
