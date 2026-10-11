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
    reading(paths).due_by(at, at, "UTC").unwrap()
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

/// Tick one occurrence off, the way the agenda does.
fn tick(paths: &StorePaths, slug: &str, occ: i64) {
    let mut s = Session::open(paths.clone()).unwrap();
    let tgt = s.state.resolve(slug).unwrap().unwrap();
    s.append(
        capture_core::event::Kind::OCC_DONE,
        Some(tgt),
        None,
        |nonce| {
            let mut b = Vec::new();
            minicbor::encode(&capture_core::body::OccBody { nonce, occ }, &mut b).unwrap();
            b
        },
    )
    .unwrap();
}

/// Move one occurrence, the way the agenda does.
fn move_occurrence(paths: &StorePaths, slug: &str, occ: i64, to: i64) {
    let mut s = Session::open(paths.clone()).unwrap();
    let tgt = s.state.resolve(slug).unwrap().unwrap();
    s.append(
        capture_core::event::Kind::OCC_MOVE,
        Some(tgt),
        None,
        |nonce| {
            let mut b = Vec::new();
            minicbor::encode(
                &capture_core::body::OccMoveBody {
                    nonce,
                    occ,
                    start_ms: to,
                    end_ms: None,
                },
                &mut b,
            )
            .unwrap();
            b
        },
    )
    .unwrap();
}

/// A one-off event that was ticked off does not announce itself.
///
/// `promotions.done` is only ever set for a task, so the events branch of
/// `due_by` carried a `done` filter that was always true. Completing a one-off
/// event is not recorded there at all — a tick writes an occurrence exception
/// against the promotion's own start, which suppressed the occurrence row and
/// left the promotion row due.
///
/// So the workshop ticked off this morning announced itself this afternoon.
#[test]
fn a_ticked_one_off_event_is_silent() {
    let scratch = Scratch::new("ticked-event");
    let paths = scratch.paths();

    let slug = capture(&paths, "the workshop");
    let start = now() - HOUR;
    at(&paths, &slug, start);
    settle(&paths);
    assert_eq!(due(&paths, now()).len(), 1, "due before anybody ticks it");

    tick(&paths, &slug, start);
    settle(&paths);

    let rows = due(&paths, now());
    assert!(
        rows.is_empty(),
        "a one-off event ticked off is not still an announcement: {rows:?}"
    );
}

/// Moving a one-off event announces it once, at the time it now starts.
///
/// It produced two rows: the promotion at the instant the event no longer
/// starts, and the occurrence at the instant it now does. Two alert keys, two
/// notifications, one event — and the first of them for a time that had been
/// deliberately changed, which is the opposite of what moving it meant.
#[test]
fn a_moved_one_off_event_announces_once_at_its_new_time() {
    let scratch = Scratch::new("moved-event");
    let paths = scratch.paths();

    let slug = capture(&paths, "the dentist");
    let start = now() - 2 * HOUR;
    at(&paths, &slug, start);
    settle(&paths);

    let moved_to = start + HOUR;
    move_occurrence(&paths, &slug, start, moved_to);
    settle(&paths);

    let rows = due(&paths, now());
    assert_eq!(
        rows.len(),
        1,
        "one event is one announcement, however it was rescheduled: {rows:?}"
    );
    assert_eq!(
        rows[0].at, moved_to,
        "and at the time it now starts, not the one it was moved away from"
    );
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

    ops.mark_alerted(&tgt.0, Some(1), 1, 0).unwrap();
    assert!(
        ops.alerted(&tgt.0, Some(1), 1).unwrap(),
        "that moment is spoken for"
    );
    assert!(
        !ops.alerted(&tgt.0, Some(2), 2).unwrap(),
        "and the next one is not — a key that could not tell two occurrences \
         apart would deliver one and swallow the rest of the week",
    );

    // Idempotent: marking twice is marking once.
    ops.mark_alerted(&tgt.0, Some(1), 1, 0).unwrap();
    assert!(ops.alerted(&tgt.0, Some(1), 1).unwrap());
}

/// Correcting a time does not silence the corrected moment.
///
/// A one-off carries no occurrence, so its delivery key used to be the note
/// alone — which made "this note has been alerted" and "this moment has been
/// alerted" the same record for exactly the notes where they come apart.
///
/// The 14:00 alert fires. The meeting moves to 16:00, or the recognizer
/// misread it and somebody corrects it. `due_by` reports the new moment
/// correctly and the delivery table says it has already been said, so 16:00
/// passes in silence and nothing anywhere explains why. The same shape for a
/// task pushed to tomorrow after today's alert fired.
///
/// The projection half of this is covered by `a_corrected_time_moves_the
/// _moment`, which never touches the delivery key — which is why it passed all
/// along.
#[test]
fn a_corrected_moment_is_not_already_delivered() {
    let scratch = Scratch::new("corrected-moment");
    let paths = scratch.paths();

    let slug = capture(&paths, "the dentist");
    at(&paths, &slug, now() - HOUR);
    settle(&paths);

    let tgt = reading(&paths).resolve(&slug).unwrap().unwrap();
    let ops = capture_store::ops::Ops::open(&paths.ops).unwrap();

    let first = now() - HOUR;
    let corrected = now() + HOUR;

    ops.mark_alerted(&tgt.0, None, first, 0).unwrap();
    assert!(
        ops.alerted(&tgt.0, None, first).unwrap(),
        "the moment that was announced stays announced"
    );
    assert!(
        !ops.alerted(&tgt.0, None, corrected).unwrap(),
        "a moment that moved is a different moment — the one that was \
         delivered is the one that no longer exists"
    );
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

/// Looking ahead does not expire a mute.
///
/// Arming the next alarm asks thirty days ahead, and the horizon was being
/// used for both halves of the question — "has this moment arrived" and "has
/// this mute expired". So a note muted for a week came back as due the moment
/// anything looked further ahead than the mute.
///
/// §2.6 makes muting the cheap way to stop something resurfacing, and it has
/// to be cheaper than destroying. A mute that silently does not apply is that
/// argument failing at the only moment it matters.
#[test]
fn a_look_ahead_does_not_expire_a_mute() {
    let scratch = Scratch::new("mute-horizon");
    let paths = scratch.paths();

    let slug = capture(&paths, "something at a time");
    at(&paths, &slug, now() - HOUR);
    settle(&paths);
    assert_eq!(due(&paths, now()).len(), 1, "due before muting");

    // Muted for a week.
    capture_store::mutate::mute_note(&paths, &slug).unwrap();
    settle(&paths);
    assert!(due(&paths, now()).is_empty(), "and silent now");

    // Thirty days ahead, which is what arming the next alarm asks.
    let store = reading(&paths);
    let ahead = store.due_by(now() + 30 * 24 * HOUR, now(), "UTC").unwrap();
    assert!(
        ahead.is_empty(),
        "still muted — the horizon says how far to look, not what time it is: {ahead:?}",
    );
}

/// Only the chosen device says things.
///
/// §2.4.1 routes time cues to the designated notifier, and nothing on any
/// delivery path consulted the setting that designates one. Its only readers
/// were `capture status` and an FFI getter, so with a phone and a desktop
/// paired every moment was announced twice and the control offered to fix that
/// did nothing at all when somebody used it.
#[test]
fn only_the_chosen_device_speaks() {
    let scratch = Scratch::new("chosen-device");
    let paths = scratch.paths();

    settle(&paths);

    let me = capture_store::Identity::load(&paths).unwrap();
    let key = me.public().to_bytes();
    let mine = capture_core::id::short_key(&key);

    // Nobody has chosen. The rule applies — one device, so it is this one —
    // and §8.9 means that is evaluated rather than written down.
    assert!(
        capture_store::notifier::speaks_here(&paths, &reading(&paths), &key, 1_800_000_000_000)
            .unwrap(),
        "an unconfigured device must still speak"
    );

    // Chosen, and it is this one. The setting holds a shortened key, so a
    // comparison by equality against the full one would silence the very
    // device somebody picked.
    capture_store::mutate::set_setting(&paths, capture_store::notifier::KEY, Some(&mine)).unwrap();
    settle(&paths);
    assert!(
        capture_store::notifier::speaks_here(&paths, &reading(&paths), &key, 1_800_000_000_000)
            .unwrap(),
        "the device that was chosen is the device that speaks"
    );

    // Chosen, and it is the other one.
    capture_store::mutate::set_setting(
        &paths,
        capture_store::notifier::KEY,
        Some("00112233445566778899aabbccddeeff"),
    )
    .unwrap();
    settle(&paths);
    assert!(
        !capture_store::notifier::speaks_here(&paths, &reading(&paths), &key, 1_800_000_000_000)
            .unwrap(),
        "a device that was not chosen stays quiet — otherwise choosing one \
         changes nothing and both announce everything"
    );
}
