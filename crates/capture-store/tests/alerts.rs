//! What has come due, and everything that must not be.
//!
//! This file runs before a notification is posted anywhere, and it is the part
//! worth getting right: the exclusions. A wrong answer here is not a missing
//! feature — it is the phone buzzing about something somebody deliberately
//! muted, or announcing the content of a note they destroyed.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "a moment is compared against a real instant"
)]

use capture_core::body::{EventFields, Promoted, TaskFields, When};
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
        let p = std::env::temp_dir().join(format!("capture-alerts-{name}-{}", std::process::id()));
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

fn settle(paths: &StorePaths) {
    let _ = Session::open(paths.clone()).unwrap();
}

fn reading(paths: &StorePaths) -> capture_store::projection::Store {
    capture_store::projection::Store::open_read(&capture_store::projection::generation_path(
        &paths.state,
        capture_store::ops::Ops::open_read(&paths.ops)
            .unwrap()
            .generation()
            .unwrap(),
    ))
    .unwrap()
}

fn due(paths: &StorePaths, at: i64) -> Vec<capture_store::projection::Due> {
    reading(paths).due_by(at, "UTC").unwrap()
}

/// An event at a named instant.
fn at(paths: &StorePaths, slug: &str, start: i64) {
    promote::correct(
        paths,
        slug,
        Promoted::Event(EventFields {
            when: When::Timed {
                start_ms: start,
                end_ms: None,
                tz: "UTC".into(),
            },
            location: None,
            attendees: Vec::new(),
        }),
    )
    .unwrap();
}

#[test]
fn a_moment_is_not_due_until_it_arrives() {
    let scratch = Scratch::new("not-yet");
    let paths = scratch.paths();

    let slug = capture(&paths, "the dentist");
    at(&paths, &slug, now() + HOUR);
    settle(&paths);

    assert!(due(&paths, now()).is_empty(), "an hour out is not due now");
    assert_eq!(
        due(&paths, now() + 2 * HOUR).len(),
        1,
        "and is due once the hour has passed",
    );
}

#[test]
fn what_is_due_says_what_was_captured() {
    let scratch = Scratch::new("says");
    let paths = scratch.paths();

    let slug = capture(&paths, "dentist thursday 2pm");
    at(&paths, &slug, now() - HOUR);
    settle(&paths);

    let d = due(&paths, now());
    assert_eq!(d.len(), 1);
    assert_eq!(
        d[0].text, "dentist thursday 2pm",
        "the capture's own text — nothing summarised, nothing added (§3.1.3)",
    );
    assert!(
        d[0].timed,
        "an event named an instant, so it is time-critical"
    );
}

#[test]
fn asking_does_not_deliver() {
    let scratch = Scratch::new("idempotent-read");
    let paths = scratch.paths();

    let slug = capture(&paths, "the dentist");
    at(&paths, &slug, now() - HOUR);
    settle(&paths);

    assert_eq!(due(&paths, now()).len(), 1);
    assert_eq!(
        due(&paths, now()).len(),
        1,
        "asking twice answers twice — `capture due` could not exist otherwise",
    );
}

#[test]
fn a_muted_note_is_silent() {
    let scratch = Scratch::new("muted");
    let paths = scratch.paths();

    let slug = capture(&paths, "the dentist");
    at(&paths, &slug, now() - HOUR);
    settle(&paths);
    assert_eq!(due(&paths, now()).len(), 1, "due before muting");

    capture_store::mutate::mute_note(&paths, &slug).unwrap();
    settle(&paths);

    assert!(
        due(&paths, now()).is_empty(),
        "muting is the escape for a dated capture that was a record (§2.6)",
    );
}

#[test]
fn a_completed_task_is_silent() {
    let scratch = Scratch::new("done");
    let paths = scratch.paths();

    let slug = capture(&paths, "call the dentist");
    promote::correct(
        &paths,
        &slug,
        Promoted::Task(TaskFields {
            done: false,
            due: Some("2020-01-01".into()),
            order: None,
        }),
    )
    .unwrap();
    settle(&paths);
    assert_eq!(due(&paths, now()).len(), 1, "an overdue task is due");

    promote::set_done(&paths, &slug, true).unwrap();
    settle(&paths);
    assert!(
        due(&paths, now()).is_empty(),
        "a finished task says nothing"
    );
}

#[test]
fn a_task_with_no_due_date_never_becomes_overdue() {
    let scratch = Scratch::new("undated");
    let paths = scratch.paths();

    let slug = capture(&paths, "call the dentist");
    promote::as_task(&paths, &slug, None).unwrap();
    settle(&paths);

    assert!(
        due(&paths, now() + 365 * 24 * HOUR).is_empty(),
        "§3.6 refuses the manufactured backlog: no date, no moment, ever",
    );
}

#[test]
fn a_due_date_is_not_time_critical() {
    let scratch = Scratch::new("floating");
    let paths = scratch.paths();

    let slug = capture(&paths, "call the dentist");
    promote::correct(
        &paths,
        &slug,
        Promoted::Task(TaskFields {
            done: false,
            due: Some("2020-01-01".into()),
            order: None,
        }),
    )
    .unwrap();
    settle(&paths);

    let d = due(&paths, now());
    assert_eq!(d.len(), 1);
    assert!(
        !d[0].timed,
        "a day was named, not a time — delivering at 00:00 obeys the letter \
         of a due date and misses what somebody meant by \"tomorrow\"",
    );
}

#[test]
fn a_redacted_note_does_not_speak_its_content() {
    let scratch = Scratch::new("redacted");
    let paths = scratch.paths();

    let slug = capture(&paths, "something private at a time");
    at(&paths, &slug, now() - HOUR);
    settle(&paths);
    assert_eq!(due(&paths, now()).len(), 1);

    capture_store::mutate::redact(&paths, &slug).unwrap();
    settle(&paths);

    assert!(
        due(&paths, now()).is_empty(),
        "an alert must not become a way to read back what redaction removed",
    );
}

#[test]
fn a_corrected_time_moves_the_moment() {
    let scratch = Scratch::new("corrected");
    let paths = scratch.paths();

    let slug = capture(&paths, "the dentist");
    at(&paths, &slug, now() - HOUR);
    settle(&paths);
    assert_eq!(due(&paths, now()).len(), 1, "due at the original time");

    // Moved into the future. Nothing had to cancel an alert, because there was
    // never an alert to cancel — the moment is derived.
    at(&paths, &slug, now() + 24 * HOUR);
    settle(&paths);

    assert!(due(&paths, now()).is_empty(), "and no longer due");
    assert_eq!(
        due(&paths, now() + 48 * HOUR).len(),
        1,
        "due at the new time"
    );
}

#[test]
fn delivery_is_remembered_per_moment_not_per_note() {
    let scratch = Scratch::new("per-moment");
    let paths = scratch.paths();

    let slug = capture(&paths, "standup");
    at(&paths, &slug, now() - HOUR);
    settle(&paths);

    let tgt = reading(&paths).resolve(&slug).unwrap().unwrap();
    let ops = capture_store::ops::Ops::open(&paths.ops).unwrap();

    ops.mark_alerted(&tgt.0, Some(1), 0).unwrap();
    assert!(
        ops.alerted(&tgt.0, Some(1)).unwrap(),
        "that moment is spoken for"
    );
    assert!(
        !ops.alerted(&tgt.0, Some(2)).unwrap(),
        "and the next one is not — a key that could not tell two occurrences \
         apart would deliver one and swallow the rest of the week",
    );

    // Idempotent: marking twice is marking once.
    ops.mark_alerted(&tgt.0, Some(1), 0).unwrap();
    assert!(ops.alerted(&tgt.0, Some(1)).unwrap());
}

#[test]
fn a_backlog_arrives_oldest_first() {
    let scratch = Scratch::new("backlog");
    let paths = scratch.paths();

    for (text, ago) in [("first", 5), ("second", 3), ("third", 1)] {
        let slug = capture(&paths, text);
        at(&paths, &slug, now() - ago * HOUR);
    }
    settle(&paths);

    let texts: Vec<_> = due(&paths, now()).into_iter().map(|d| d.text).collect();
    assert_eq!(
        texts,
        ["first", "second", "third"],
        "a day delivered in the order it happened reads as a day, not a pile",
    );
}

/// Whatever is captured, nothing silenced is ever due.
///
/// Stated over arbitrary combinations rather than examples because the
/// exclusions are four independent conditions and the ways they interact are
/// more numerous than anybody writes down. A wrong answer here is not a
/// missing feature: it is a phone announcing something somebody deliberately
/// muted, or reading out a note they destroyed.
#[test]
fn nothing_silenced_is_ever_due() {
    // Every combination of the four states a dated capture can be left in.
    for muted in [false, true] {
        for done in [false, true] {
            for redacted in [false, true] {
                let name = format!("m{muted}d{done}r{redacted}");
                let scratch = Scratch::new(&name);
                let paths = scratch.paths();

                let slug = capture(&paths, &format!("something at a time {name}"));
                promote::correct(
                    &paths,
                    &slug,
                    Promoted::Task(TaskFields {
                        done: false,
                        due: Some("2020-01-01".into()),
                        order: None,
                    }),
                )
                .unwrap();
                settle(&paths);

                // Overdue by years, so the only thing that can keep it quiet
                // is one of the states below.
                assert_eq!(due(&paths, now()).len(), 1, "{name}: due before anything");

                if muted {
                    capture_store::mutate::mute_note(&paths, &slug).unwrap();
                }
                if done {
                    promote::set_done(&paths, &slug, true).unwrap();
                }
                if redacted {
                    capture_store::mutate::redact(&paths, &slug).unwrap();
                }
                settle(&paths);

                let silenced = muted || done || redacted;
                let found = due(&paths, now());
                if silenced {
                    assert!(
                        found.is_empty(),
                        "{name}: silenced and still due — {found:?}",
                    );
                } else {
                    assert_eq!(found.len(), 1, "{name}: nothing silenced it");
                }
            }
        }
    }
}

/// What is due is always a subset of what exists, and never invented.
#[test]
fn nothing_is_due_that_was_not_captured() {
    let scratch = Scratch::new("subset");
    let paths = scratch.paths();

    let mut want = Vec::new();
    for i in 0..6 {
        let text = format!("thing number {i}");
        let slug = capture(&paths, &text);
        // Half dated, half not. The undated ones must never appear, however
        // far ahead the question is asked.
        if i % 2 == 0 {
            at(&paths, &slug, now() - HOUR);
            want.push(text);
        }
    }
    settle(&paths);

    for ahead in [0, HOUR, 24 * HOUR, 365 * 24 * HOUR] {
        let found: Vec<_> = due(&paths, now() + ahead)
            .into_iter()
            .map(|d| d.text)
            .collect();
        assert_eq!(
            found.len(),
            want.len(),
            "at +{ahead}ms: an undated capture became due, or a dated one vanished",
        );
        for t in &found {
            assert!(want.contains(t), "{t:?} was never given a time");
        }
    }
}
