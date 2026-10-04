//! Occurrence expansion, from a capture to rows a calendar can draw.
//!
//! The unit tests in `capture-time` prove the expansion itself is right. These
//! prove the projection stores what it produced, applies the exceptions on top,
//! and throws all of it away and recomputes it on a rebuild — which is what
//! makes occurrences derived data rather than a second truth (§3.4).

use capture_core::body::{Promoted, RuleSpec, SeriesFields, When};
use capture_core::event::Kind;
use capture_store::session::Session;
use capture_store::{StorePaths, promote};

/// Monday 1 June 2026, 09:00 in Denver.
const NOW: i64 = 1_780_326_000_000;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("capture-occ-{name}-{}", std::process::id()));
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

/// A capture promoted by hand to a weekday series at 07:00.
fn weekday_series(paths: &StorePaths) -> (String, capture_core::EventId) {
    let mut s = Session::open(paths.clone()).unwrap();
    let body = capture_store::session::capture_body("walk the dog every weekday at 7").unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);

    promote::correct(
        paths,
        &slug,
        Promoted::Series(SeriesFields {
            when: When::Timed {
                start_ms: NOW,
                end_ms: None,
                tz: "America/Denver".into(),
            },
            rule: RuleSpec {
                freq: 1,
                interval: 1,
                by_day: 0b0001_1111,
                by_month_day: Vec::new(),
                count: None,
                until: None,
            },
            carries_forward: false,
            split_from: None,
        }),
    )
    .unwrap();

    let s = Session::open(paths.clone()).unwrap();
    let tgt = s.state.resolve(&slug).unwrap().unwrap();
    (slug, tgt)
}

fn rebuild(paths: &StorePaths, now: i64) -> capture_store::projection::Store {
    let s = Session::open(paths.clone()).unwrap();
    let all = s.log.all().unwrap();
    let generation = capture_store::ops::Ops::open(&paths.ops)
        .unwrap()
        .generation()
        .unwrap();
    let next = capture_store::projection::rebuild(&paths.state, &all, generation, now).unwrap();
    capture_store::ops::Ops::open(&paths.ops)
        .unwrap()
        .set_generation(next)
        .unwrap();
    drop(s);
    capture_store::projection::Store::open_read(&capture_store::projection::generation_path(
        &paths.state,
        next,
    ))
    .unwrap()
}

#[test]
fn a_series_expands_into_occurrences() {
    let sc = Scratch::new("expand");
    let p = sc.paths();
    let (_, tgt) = weekday_series(&p);

    let store = rebuild(&p, NOW);
    let occ = store.occurrences_of(&tgt).unwrap();

    // Roughly 260: a weekday series starting today reaches one year forward,
    // and the backward half of the horizon has nothing to expand because the
    // series did not exist yet. The horizon bounds the window, not the series.
    assert!(
        (250..=270).contains(&occ.len()),
        "expected about a year of weekdays, got {}",
        occ.len()
    );
    assert!(occ.windows(2).all(|w| w[0].start_ms <= w[1].start_ms));
}

#[test]
fn an_occurrence_is_named_by_where_the_rule_put_it() {
    // Its identity, which a move must not change.
    let sc = Scratch::new("named");
    let p = sc.paths();
    let (_, tgt) = weekday_series(&p);

    let store = rebuild(&p, NOW);
    for o in store.occurrences_of(&tgt).unwrap() {
        assert_eq!(o.occ, o.start_ms, "nothing was moved, so these must agree");
    }
}

#[test]
fn a_rebuild_reproduces_the_expansion_exactly() {
    // Task 6.5. Occurrences are derived: a rebuild discards every row and
    // recomputes it, which is what makes a timezone database update a re-fold
    // rather than a migration.
    let sc = Scratch::new("rebuild");
    let p = sc.paths();
    let (_, tgt) = weekday_series(&p);

    let first: Vec<_> = rebuild(&p, NOW).occurrences_of(&tgt).unwrap();
    let second: Vec<_> = rebuild(&p, NOW).occurrences_of(&tgt).unwrap();
    assert_eq!(first, second, "a rebuild produced a different expansion");
    assert!(!first.is_empty(), "the comparison would be vacuous");
}

#[test]
fn an_exception_lands_on_the_occurrence_it_names() {
    let sc = Scratch::new("exception");
    let p = sc.paths();
    let (_, tgt) = weekday_series(&p);

    let target = rebuild(&p, NOW).occurrences_of(&tgt).unwrap()[3].occ;

    let mut s = Session::open(p.clone()).unwrap();
    s.append(Kind::OCC_DONE, Some(tgt), None, |nonce| {
        let mut b = Vec::new();
        minicbor::encode(&capture_core::body::OccBody { nonce, occ: target }, &mut b).unwrap();
        b
    })
    .unwrap();
    drop(s);

    let store = rebuild(&p, NOW);
    let occ = store.occurrences_of(&tgt).unwrap();
    let hit = occ.iter().find(|o| o.occ == target).expect("still there");
    assert!(hit.done, "the completion did not reach its occurrence");
    assert_eq!(
        occ.iter().filter(|o| o.done).count(),
        1,
        "a completion reached more than one occurrence"
    );
}

#[test]
fn a_moved_occurrence_keeps_its_name_and_moves_its_time() {
    let sc = Scratch::new("moved");
    let p = sc.paths();
    let (_, tgt) = weekday_series(&p);

    let target = rebuild(&p, NOW).occurrences_of(&tgt).unwrap()[2].occ;
    let moved_to = target + 3 * 60 * 60 * 1000;

    let mut s = Session::open(p.clone()).unwrap();
    s.append(Kind::OCC_MOVE, Some(tgt), None, |nonce| {
        let mut b = Vec::new();
        minicbor::encode(
            &capture_core::body::OccMoveBody {
                nonce,
                occ: target,
                start_ms: moved_to,
                end_ms: None,
            },
            &mut b,
        )
        .unwrap();
        b
    })
    .unwrap();
    drop(s);

    let store = rebuild(&p, NOW);
    let hit = store
        .occurrences_of(&tgt)
        .unwrap()
        .into_iter()
        .find(|o| o.occ == target)
        .expect("a move must not rename an occurrence");
    assert_eq!(hit.start_ms, moved_to);
}

#[test]
fn a_calendar_query_is_answered_from_rows_rather_than_text() {
    let sc = Scratch::new("between");
    let p = sc.paths();
    weekday_series(&p);

    let store = rebuild(&p, NOW);
    let week = store
        .occurring_between(NOW, NOW + 7 * 24 * 60 * 60 * 1000)
        .unwrap();
    assert_eq!(week.len(), 5, "a week of weekdays is five days");
}

// ------------------------------------------------------------- splitting ----

#[test]
fn a_split_leaves_the_past_exactly_as_it_happened() {
    // §3.5. Editing the rule in place would rewrite what the schedule *was*,
    // and a log that revises its own history is an unreliable narrator.
    let sc = Scratch::new("split");
    let p = sc.paths();
    let (slug, tgt) = weekday_series(&p);

    let before = rebuild(&p, NOW).occurrences_of(&tgt).unwrap();
    let boundary = before[10].occ;
    let past: Vec<i64> = before
        .iter()
        .map(|o| o.occ)
        .filter(|o| *o < boundary)
        .collect();

    // From the boundary onward: weekends instead of weekdays.
    promote::split_series(
        &p,
        &slug,
        boundary,
        RuleSpec {
            freq: 1,
            interval: 1,
            by_day: 0b0110_0000,
            by_month_day: Vec::new(),
            count: None,
            until: None,
        },
    )
    .unwrap();

    let store = rebuild(&p, NOW);
    let after: Vec<i64> = store
        .occurrences_of(&tgt)
        .unwrap()
        .into_iter()
        .map(|o| o.occ)
        .collect();

    assert_eq!(after, past, "the past changed when the future did");
    assert!(
        !after.contains(&boundary),
        "the old series still produces the occurrence it was bounded before"
    );
}

#[test]
fn the_new_series_carries_on_from_the_split() {
    let sc = Scratch::new("split-new");
    let p = sc.paths();
    let (slug, tgt) = weekday_series(&p);
    let boundary = rebuild(&p, NOW).occurrences_of(&tgt).unwrap()[10].occ;

    promote::split_series(
        &p,
        &slug,
        boundary,
        RuleSpec {
            freq: 1,
            interval: 1,
            by_day: 0b0110_0000,
            by_month_day: Vec::new(),
            count: None,
            until: None,
        },
    )
    .unwrap();

    let store = rebuild(&p, NOW);
    let series = store.promoted("series", 100).unwrap();
    assert_eq!(series.len(), 2, "a split should leave two series");

    // The continuation occurs, and does so on weekends.
    let new = series
        .iter()
        .find(|(id, _)| *id != tgt)
        .expect("the new series exists");
    let occ = store.occurrences_of(&new.0).unwrap();
    assert!(!occ.is_empty(), "the continuation expanded to nothing");
    assert!(occ.iter().all(|o| o.start_ms >= boundary));
}

/// Delivery survives a split, which is the other half of §3.5's inheritance.
///
/// Splitting gives every occurrence from the boundary onward a new note, and
/// delivery is keyed by the note — so all of them read as never announced. The
/// boundary is the one that bites: the same instant and the same text as one
/// already said. "Change standup from today onward", done at four in the
/// afternoon, re-announced this morning's standup.
///
/// The expansion already inherits exceptions across a split, for the reason
/// that what somebody did to an occurrence survives a change to the schedule
/// it sits on. This is the same principle applied to what the archive already
/// told them.
#[test]
fn delivery_survives_a_split() {
    let sc = Scratch::new("split-delivery");
    let p = sc.paths();
    let (slug, tgt) = weekday_series(&p);

    let before = rebuild(&p, NOW).occurrences_of(&tgt).unwrap();
    let boundary = before[10].occ;

    // Every one of them announced, the way a week of standups would be.
    let ops = capture_store::ops::Ops::open(&p.ops).unwrap();
    for o in &before {
        ops.mark_alerted(&tgt.0, Some(o.occ), o.occ, 0).unwrap();
    }
    drop(ops);

    promote::split_series(
        &p,
        &slug,
        boundary,
        RuleSpec {
            freq: 1,
            interval: 1,
            by_day: 0b0110_0000,
            by_month_day: Vec::new(),
            count: None,
            until: None,
        },
    )
    .unwrap();

    // The note the boundary now belongs to.
    let store = rebuild(&p, NOW);
    let new_tgt = store
        .promoted("series", 100)
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .find(|t| *t != tgt)
        .expect("the split made a second series");

    let ops = capture_store::ops::Ops::open(&p.ops).unwrap();
    assert!(
        ops.alerted(&new_tgt.0, Some(boundary), boundary).unwrap(),
        "the boundary occurrence was announced before the split and is the \
         same instant afterwards — changing the schedule is not news about it"
    );
}

#[test]
fn a_completion_survives_a_split_that_moves_past_it() {
    // The fiddly half of §3.5: exceptions bind to whichever series covers their
    // original start. Complete an occurrence, then change the schedule from
    // before it — the completion must still be there.
    let sc = Scratch::new("split-exception");
    let p = sc.paths();
    let (slug, tgt) = weekday_series(&p);

    let occ = rebuild(&p, NOW).occurrences_of(&tgt).unwrap();
    let boundary = occ[10].occ;
    let completed = occ[12].occ;

    let mut s = Session::open(p.clone()).unwrap();
    s.append(Kind::OCC_DONE, Some(tgt), None, |nonce| {
        let mut b = Vec::new();
        minicbor::encode(
            &capture_core::body::OccBody {
                nonce,
                occ: completed,
            },
            &mut b,
        )
        .unwrap();
        b
    })
    .unwrap();
    drop(s);

    // Split before the completed occurrence, keeping the same weekday rule so
    // the continuation still produces it.
    promote::split_series(
        &p,
        &slug,
        boundary,
        RuleSpec {
            freq: 1,
            interval: 1,
            by_day: 0b0001_1111,
            by_month_day: Vec::new(),
            count: None,
            until: None,
        },
    )
    .unwrap();

    let store = rebuild(&p, NOW);
    let new = store
        .promoted("series", 100)
        .unwrap()
        .into_iter()
        .find(|(id, _)| *id != tgt)
        .expect("the continuation exists");

    let hit = store
        .occurrences_of(&new.0)
        .unwrap()
        .into_iter()
        .find(|o| o.occ == completed);

    assert!(
        hit.is_some_and(|o| o.done),
        "a completion was lost when the schedule moved past it"
    );
}

/// The one question review asks, and the answer being a person's to give.
///
/// §3.6 makes this the foot-gun exception: "walk the dog daily" and "take the
/// pill daily" are the same sentence to a parser and opposite answers to
/// somebody living with the consequences. So the flag is only ever set by a
/// person, and it lives in the store rather than in a surface because there are
/// now two surfaces asking for it — the command line and the review pane.
#[test]
fn a_person_can_say_that_missing_an_instance_leaves_something_owed() {
    let scratch = Scratch::new("carries");
    let paths = scratch.paths();
    let (slug, tgt) = weekday_series(&paths);

    let before = {
        let s = Session::open(paths.clone()).unwrap();
        match s.state.promotion_of(&tgt).unwrap().map(|p| p.as_) {
            Some(Promoted::Series(f)) => f.carries_forward,
            other => panic!("expected a series, got {other:?}"),
        }
    };
    assert!(!before, "nothing may carry forward until somebody says so");

    promote::carries_forward(&paths, &slug).unwrap();

    let s = Session::open(paths.clone()).unwrap();
    match s.state.promotion_of(&tgt).unwrap().map(|p| p.as_) {
        Some(Promoted::Series(f)) => assert!(f.carries_forward),
        other => panic!("expected a series, got {other:?}"),
    }

    // Pinned, like every correction: a later reparse must not take it back off.
    // A flag that silently reverted would be worse than never having been set,
    // because the user would have been told it was.
    assert!(
        s.state.promotion_of(&tgt).unwrap().unwrap().pinned,
        "a person's answer must not be displaced by a reparse"
    );
}

#[test]
fn nothing_but_a_series_can_owe_you_anything() {
    // A one-off cannot lapse repeatedly, so there is nothing for the flag to
    // mean. Refused rather than accepted and ignored — a setting that appears
    // to take and does nothing is the shape of failure §2.4.1 spends its length
    // on.
    let scratch = Scratch::new("carries-nonseries");
    let paths = scratch.paths();

    let mut s = Session::open(paths.clone()).unwrap();
    let body = capture_store::session::capture_body("dentist on friday").unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);
    promote::correct(&paths, &slug, promote::task(false, None)).unwrap();

    let e = promote::carries_forward(&paths, &slug).unwrap_err();
    assert!(
        e.to_string().contains("not a recurring series"),
        "unhelpful refusal: {e}"
    );
}

// ---------------------------------------------------------------------------
// Tasks. §3.6's table is the whole of this: a task has state and occupies no
// time, where an event occupies time and has none.
// ---------------------------------------------------------------------------

#[test]
fn a_task_is_due_on_a_day_rather_than_occurring_on_it() {
    // The agenda is "what is due and what occurs" (§4.4). Until `tasks_due_on`
    // existed it answered only the second half, so a task due today was listed
    // by `capture tasks` and invisible on the day it mattered — the same failure
    // shape as an intent that is registered and never fires.
    let scratch = Scratch::new("task-due");
    let paths = scratch.paths();

    let mut s = Session::open(paths.clone()).unwrap();
    let body = capture_store::session::capture_body("call the dentist").unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);

    promote::as_task(&paths, &slug, Some("2026-06-05")).unwrap();

    let s = Session::open(paths.clone()).unwrap();
    let due = s.state.tasks_due_on("2026-06-05").unwrap();
    assert_eq!(due.len(), 1, "the task is not due on the day it says");
    assert!(!due[0].1, "a new task is not finished");
    assert!(
        s.state.tasks_due_on("2026-06-04").unwrap().is_empty(),
        "a due date must not spill onto neighbouring days"
    );

    // It does not occur. Nothing was expanded, because a task occupies no time.
    let from = capture_time::at_local(
        capture_time::FloatingDate::parse("2026-06-05").unwrap(),
        0,
        0,
        capture_time::Zone::new("UTC").unwrap(),
    )
    .unwrap();
    assert!(
        s.state
            .happening_between(from, from + 86_400_000)
            .unwrap()
            .is_empty(),
        "a task appeared on the hour axis, which is what §3.6 says it has none of"
    );
}

#[test]
fn a_finished_task_stops_being_due() {
    let scratch = Scratch::new("task-done");
    let paths = scratch.paths();

    let mut s = Session::open(paths.clone()).unwrap();
    let body = capture_store::session::capture_body("pay the invoice").unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);

    promote::as_task(&paths, &slug, Some("2026-06-05")).unwrap();
    promote::set_done(&paths, &slug, true).unwrap();

    let s = Session::open(paths.clone()).unwrap();

    // Still listed on its own day, and flagged. Not because a finished thing is
    // still due — it is not, and `tasks_overdue` refuses it below — but because
    // a task that vanished the instant it was ticked would leave a mistaken
    // tick reversible only by hunting for the note, where an occurrence stays
    // put with a mark against it and un-ticks in one gesture.
    match s.state.tasks_due_on("2026-06-05").unwrap().as_slice() {
        [(_, done)] => assert!(*done, "a finished task is not reported as finished"),
        other => panic!("expected the finished task on its own day, got {other:?}"),
    }

    // And the due date survived the completion. Reading the fields and writing
    // them back is what makes that true; replacing them would have dropped it.
    let tgt = s.state.resolve(&slug).unwrap().unwrap();
    match s.state.promotion_of(&tgt).unwrap().map(|p| p.as_) {
        Some(Promoted::Task(t)) => {
            assert!(t.done);
            assert_eq!(t.due.as_deref(), Some("2026-06-05"));
        }
        other => panic!("expected a finished task, got {other:?}"),
    }
    drop(s);

    // Reversible from anywhere, because a mistaken tick is the ordinary case.
    promote::set_done(&paths, &slug, false).unwrap();
    let s = Session::open(paths.clone()).unwrap();
    match s.state.tasks_due_on("2026-06-05").unwrap().as_slice() {
        [(_, done)] => assert!(!*done, "un-ticking did not reopen it"),
        other => panic!("expected one open task, got {other:?}"),
    }
}

#[test]
fn an_open_task_whose_day_has_passed_is_overdue_rather_than_lapsed() {
    // §3.6 draws the line: a task is the one thing here that genuinely carries
    // forward — it is not finished — where a missed occurrence of a series
    // lapses and nothing accumulates.
    let scratch = Scratch::new("task-overdue");
    let paths = scratch.paths();

    let mut s = Session::open(paths.clone()).unwrap();
    let body = capture_store::session::capture_body("renew the domain").unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);

    promote::as_task(&paths, &slug, Some("2026-05-30")).unwrap();

    let s = Session::open(paths.clone()).unwrap();
    assert_eq!(
        s.state.tasks_overdue("2026-06-01").unwrap().len(),
        1,
        "an open task past its day is not reported"
    );
    assert!(
        s.state.tasks_due_on("2026-06-01").unwrap().is_empty(),
        "an overdue task must not also be reported as due today"
    );
    assert!(
        s.state.tasks_overdue("2026-05-29").unwrap().is_empty(),
        "a task became overdue before its own due date"
    );
}

#[test]
fn a_due_date_that_is_not_a_date_is_refused() {
    // Refused where it is written rather than stored and discovered later by a
    // query that silently matches nothing.
    let scratch = Scratch::new("task-baddate");
    let paths = scratch.paths();

    let mut s = Session::open(paths.clone()).unwrap();
    let body = capture_store::session::capture_body("book the flights").unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);

    let e = promote::as_task(&paths, &slug, Some("tuesday")).unwrap_err();
    assert!(
        e.to_string().contains("not a date"),
        "unhelpful refusal: {e}"
    );
}

#[test]
fn a_finished_task_does_not_follow_you_to_tomorrow() {
    // The other half of keeping it visible. It stays on its own day so a
    // mistaken tick is one gesture from being undone; it does not accumulate
    // onto every day after, which is the manufactured backlog §3.6 refuses.
    let scratch = Scratch::new("task-not-forward");
    let paths = scratch.paths();

    let mut s = Session::open(paths.clone()).unwrap();
    let body = capture_store::session::capture_body("file the return").unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);

    promote::as_task(&paths, &slug, Some("2026-06-05")).unwrap();
    promote::set_done(&paths, &slug, true).unwrap();

    let s = Session::open(paths.clone()).unwrap();
    assert!(s.state.tasks_due_on("2026-06-06").unwrap().is_empty());
    assert!(
        s.state.tasks_overdue("2026-06-30").unwrap().is_empty(),
        "a finished task became overdue"
    );
}

#[test]
fn ticking_a_one_off_event_does_not_double_it() {
    // Found in a screenshot of a real day: "standup at 9am" drawn twice side by
    // side, and "workshop at 1pm" drawn twice with only one of them ticked.
    //
    // A one-off event lives in `promotions`. It gains a row in `occurrences`
    // only when somebody writes an exception against it — ticking it done is
    // the usual way — and from that moment `happening_between` reported it
    // twice: once from the expansion carrying the completion, and once from the
    // promotions fallback with `done: false` hardcoded, so the tick looked as
    // though it had not taken.
    //
    // Only the events somebody had actually interacted with doubled, which is
    // exactly why no test caught it: every one of them ticked a series.
    let scratch = Scratch::new("one-off-tick");
    let paths = scratch.paths();

    let mut s = Session::open(paths.clone()).unwrap();
    let body = capture_store::session::capture_body("standup at 9am").unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);

    promote::correct(
        &paths,
        &slug,
        Promoted::Event(capture_core::body::EventFields {
            when: When::Timed {
                start_ms: NOW,
                end_ms: None,
                tz: "America/Denver".into(),
            },
            location: None,
            attendees: Vec::new(),
        }),
    )
    .unwrap();

    let day = |s: &Session| {
        s.state
            .happening_between(NOW - 3_600_000, NOW + 3_600_000)
            .unwrap()
    };

    let s = Session::open(paths.clone()).unwrap();
    assert_eq!(day(&s).len(), 1, "an untouched event was already doubled");
    let tgt = s.state.resolve(&slug).unwrap().unwrap();
    drop(s);

    // Tick it, the way the window does.
    let mut s = Session::open(paths.clone()).unwrap();
    s.append(Kind::OCC_DONE, Some(tgt), None, |nonce| {
        let mut b = Vec::new();
        minicbor::encode(&capture_core::body::OccBody { nonce, occ: NOW }, &mut b).unwrap();
        b
    })
    .unwrap();
    drop(s);

    let s = Session::open(paths.clone()).unwrap();
    let rows = day(&s);
    assert_eq!(rows.len(), 1, "ticking it drew it twice: {rows:?}");
    assert!(
        rows[0].1.done,
        "the tick did not show on the row that survived"
    );
}

#[test]
fn ticking_an_event_does_not_shorten_it() {
    // The second half of the same screenshot, and a bug the first fix exposed
    // rather than caused: with the duplicate gone, the row that survived was
    // the exception's, and that row was written with no end at all. A two-hour
    // workshop became a mark on the grid the moment it was ticked.
    let scratch = Scratch::new("one-off-span");
    let paths = scratch.paths();

    let mut s = Session::open(paths.clone()).unwrap();
    let body = capture_store::session::capture_body("workshop at 1pm").unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);

    let ends = NOW + 2 * 3_600_000;
    promote::correct(
        &paths,
        &slug,
        Promoted::Event(capture_core::body::EventFields {
            when: When::Timed {
                start_ms: NOW,
                end_ms: Some(ends),
                tz: "America/Denver".into(),
            },
            location: None,
            attendees: Vec::new(),
        }),
    )
    .unwrap();

    let s = Session::open(paths.clone()).unwrap();
    let tgt = s.state.resolve(&slug).unwrap().unwrap();
    drop(s);

    let mut s = Session::open(paths.clone()).unwrap();
    s.append(Kind::OCC_DONE, Some(tgt), None, |nonce| {
        let mut b = Vec::new();
        minicbor::encode(&capture_core::body::OccBody { nonce, occ: NOW }, &mut b).unwrap();
        b
    })
    .unwrap();
    drop(s);

    let s = Session::open(paths.clone()).unwrap();
    let rows = s
        .state
        .happening_between(NOW - 3_600_000, NOW + 3_600_000)
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].1.end_ms,
        Some(ends),
        "ticking it threw away how long it was"
    );
}

// ------------------------------------------------- skipping one ------------
//
// `occ_cancel` and `occ_move` had a frozen kind, a body, a fold arm, a
// projection column, a conflict rule and tests, and nothing emitted them. The
// browse spec meanwhile described a cancelled occurrence as "hidden but not
// gone", which is a state no surface could produce.

#[test]
fn skipping_one_occurrence_leaves_the_rest_of_the_series_alone() {
    let sc = Scratch::new("skip");
    let p = sc.paths();
    let (name, tgt) = weekday_series(&p);

    let before = rebuild(&p, NOW).occurrences_of(&tgt).unwrap();
    let target = before[2].occ;

    capture_store::mutate::skip_occurrence(&p, &name, target).unwrap();

    let after = rebuild(&p, NOW).occurrences_of(&tgt).unwrap();
    assert_eq!(
        after.len(),
        before.len(),
        "the rule did not change, so the occurrences it produces did not either"
    );
    for (a, b) in after.iter().zip(before.iter()) {
        assert_eq!(
            a.occ, b.occ,
            "an occurrence was renamed by skipping another"
        );
        if a.occ == target {
            assert!(a.cancelled, "the one that was skipped is flagged");
        } else {
            assert!(!a.cancelled, "only the named occurrence is skipped");
        }
    }
}

#[test]
fn a_restored_occurrence_is_visible_again() {
    // The bug this found: `OccState::visible()` says a move beats a cancel, and
    // `occurrences_of`'s own comment claims the same — but the expansion wrote
    // `cancelled` straight through and ignored `moved_to`. So the rule was true
    // in the fold and absent from the column every surface filters on, and the
    // designed recovery gesture did nothing. Found by trying to use it.
    let sc = Scratch::new("restore");
    let p = sc.paths();
    let (name, tgt) = weekday_series(&p);
    let target = rebuild(&p, NOW).occurrences_of(&tgt).unwrap()[2].occ;

    capture_store::mutate::skip_occurrence(&p, &name, target).unwrap();
    assert!(
        at(&rebuild(&p, NOW), &tgt, target).cancelled,
        "it should be skipped first, or this proves nothing"
    );

    capture_store::mutate::restore_occurrence(&p, &name, target).unwrap();

    let row = at(&rebuild(&p, NOW), &tgt, target);
    assert!(
        !row.cancelled,
        "a move beats a cancel, and nothing read that"
    );
    assert_eq!(
        row.start_ms, target,
        "restoring puts it back where the rule had it"
    );
}

#[test]
fn skipping_is_idempotent_and_so_is_restoring() {
    // Both are statements about a state rather than toggles. A refusal that
    // depended on the current state would make a gesture somebody has to check
    // before using.
    let sc = Scratch::new("idempotent");
    let p = sc.paths();
    let (name, tgt) = weekday_series(&p);
    let target = rebuild(&p, NOW).occurrences_of(&tgt).unwrap()[2].occ;

    capture_store::mutate::skip_occurrence(&p, &name, target).unwrap();
    capture_store::mutate::skip_occurrence(&p, &name, target).unwrap();
    assert!(at(&rebuild(&p, NOW), &tgt, target).cancelled);

    capture_store::mutate::restore_occurrence(&p, &name, target).unwrap();
    capture_store::mutate::restore_occurrence(&p, &name, target).unwrap();
    assert!(!at(&rebuild(&p, NOW), &tgt, target).cancelled);
}

#[test]
fn the_cancellation_stays_in_the_log_after_a_restore() {
    // "Hidden but not gone" is the spec's phrase, and it is about the log
    // rather than the view: nothing is destroyed, the move simply supersedes.
    let sc = Scratch::new("kept");
    let p = sc.paths();
    let (name, tgt) = weekday_series(&p);
    let target = rebuild(&p, NOW).occurrences_of(&tgt).unwrap()[2].occ;

    capture_store::mutate::skip_occurrence(&p, &name, target).unwrap();
    capture_store::mutate::restore_occurrence(&p, &name, target).unwrap();

    let log = capture_store::log::Log::open_read(&p.events).unwrap();
    let cancels = log
        .all()
        .unwrap()
        .iter()
        .filter(|e| e.header.kind == Kind::OCC_CANCEL)
        .count();
    assert_eq!(
        cancels, 1,
        "the cancellation was rewritten rather than kept"
    );
}

#[test]
fn a_note_with_no_occurrences_is_not_a_series() {
    // An exception naming a plain note would sit in the log for ever recording
    // a mistake that changes nothing when folded — the system silently right
    // and the person silently wrong.
    let sc = Scratch::new("notaseries");
    let p = sc.paths();
    let slug = {
        let mut s = Session::open(p.clone()).unwrap();
        let body = capture_store::session::capture_body("an ordinary note").unwrap();
        s.capture(&body).unwrap().slug
    };
    promote::settle(&p).unwrap();

    let e = capture_store::mutate::skip_occurrence(&p, &slug, 0).unwrap_err();
    assert!(
        format!("{e}").contains("no occurrences"),
        "it should say what was expected: {e}"
    );
}

/// One occurrence of a series, by its original start.
fn at(
    store: &capture_store::projection::Store,
    tgt: &capture_core::EventId,
    occ: i64,
) -> capture_store::projection::OccurrenceRow {
    store
        .occurrences_of(tgt)
        .unwrap()
        .into_iter()
        .find(|r| r.occ == occ)
        .expect("the occurrence is still produced by the rule")
}
