//! The calendar read: a range of days and what happens on each.
//!
//! One read for every surface, so the phone's month grid, the desktop's and
//! `capture cal` cannot disagree about a day. Dates are chosen relative to the
//! real clock because the occurrence horizon is (§3.4: ±1 year from now).

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "the occurrence horizon is measured from the real now"
)]

use capture_core::body::{EventFields, Promoted, RuleSpec, SeriesFields, TaskFields, When};
use capture_store::calendar::{Day, ItemKind, days};
use capture_store::session::Session;
use capture_store::{StorePaths, mutate, promote};
use capture_time::{FloatingDate, Zone};

const ZONE: &str = "America/Chicago";
const HOUR: i64 = 60 * 60 * 1000;

fn zone() -> Zone {
    Zone::new(ZONE).unwrap()
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("capture-cal-{name}-{}", std::process::id()));
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

/// A capture, given exactly the fields a test wants.
fn note(paths: &StorePaths, text: &str, as_: Promoted) -> String {
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = capture_store::session::capture_body(text).unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);
    promote::correct(paths, &slug, as_).unwrap();
    slug
}

/// A Monday at least a week away, so last week and next are both in range.
fn a_monday() -> FloatingDate {
    capture_time::next_weekday_on_or_after(now() + 7 * 24 * HOUR, zone(), 0).unwrap()
}

fn plus(d: FloatingDate, n: i64) -> FloatingDate {
    let noon = capture_time::at_local(d, 12, 0, zone()).unwrap();
    capture_time::add_days(noon, zone(), n).unwrap()
}

fn at(d: FloatingDate, hour: u32) -> i64 {
    capture_time::at_local(d, hour, 0, zone()).unwrap()
}

fn weekly_gym(paths: &StorePaths, monday: FloatingDate) -> String {
    note(
        paths,
        "gym every monday at 2pm",
        Promoted::Series(SeriesFields {
            when: When::Timed {
                // Four weeks back, so "last Monday" is always one of its own.
                start_ms: at(plus(monday, -28), 14),
                end_ms: Some(at(plus(monday, -28), 15)),
                tz: ZONE.into(),
            },
            rule: RuleSpec {
                freq: 1,
                interval: 1,
                by_day: 0b0000_0001,
                by_month_day: Vec::new(),
                count: None,
                until: None,
            },
            carries_forward: false,
            split_from: None,
        }),
    )
}

fn read(paths: &StorePaths, from: FloatingDate, to: FloatingDate) -> Vec<Day> {
    let _ = Session::open(paths.clone()).unwrap();
    let store = capture_store::projection::current(paths).unwrap();
    days(&store, from, to, zone(), now()).unwrap()
}

fn on(days: &[Day], d: FloatingDate) -> &Day {
    days.iter()
        .find(|x| x.date == d)
        .expect("the day is in the range")
}

#[test]
fn a_week_lists_each_day_and_what_happens_on_it() {
    let scratch = Scratch::new("week");
    let paths = scratch.paths();
    let monday = a_monday();
    let gym = weekly_gym(&paths, monday);
    let dentist = note(
        &paths,
        "dentist thursday 2pm",
        Promoted::Event(EventFields {
            when: When::Timed {
                start_ms: at(plus(monday, 3), 14),
                end_ms: None,
                tz: ZONE.into(),
            },
            location: None,
            attendees: Vec::new(),
        }),
    );
    let rent = note(
        &paths,
        "pay rent",
        Promoted::Task(TaskFields {
            done: false,
            due: Some(plus(monday, 1).to_string()),
            order: None,
        }),
    );

    let week = read(&paths, monday, plus(monday, 6));
    assert_eq!(week.len(), 7, "every day in the range, empty ones included");

    let mon = on(&week, monday);
    let g = mon
        .items
        .iter()
        .find(|i| i.slug == gym)
        .expect("the gym on Monday");
    assert_eq!(g.kind, ItemKind::Timed);
    assert_eq!(g.start_ms, at(monday, 14));
    assert!(
        g.series,
        "a series says so, so a surface can offer this-and-later"
    );
    assert_eq!(g.occ, Some(at(monday, 14)));
    assert_eq!(g.moved_from, None);

    let tue = on(&week, plus(monday, 1));
    let r = tue
        .items
        .iter()
        .find(|i| i.slug == rent)
        .expect("rent due Tuesday");
    assert_eq!(r.kind, ItemKind::Due);

    let thu = on(&week, plus(monday, 3));
    let d = thu
        .items
        .iter()
        .find(|i| i.slug == dentist)
        .expect("the dentist on Thursday");
    assert!(!d.series);

    assert!(
        on(&week, plus(monday, 2)).items.is_empty(),
        "Wednesday has nothing"
    );
}

#[test]
fn a_moved_occurrence_is_on_its_new_day_and_says_where_from() {
    let scratch = Scratch::new("moved");
    let paths = scratch.paths();
    let monday = a_monday();
    let gym = weekly_gym(&paths, monday);

    // Sick on Monday: Thursday at 10, just this once.
    let thursday = plus(monday, 3);
    mutate::move_occurrence(
        &paths,
        &gym,
        at(monday, 14),
        at(thursday, 10),
        Some(at(thursday, 11)),
    )
    .unwrap();

    let week = read(&paths, monday, plus(monday, 7));
    assert!(
        !on(&week, monday).items.iter().any(|i| i.slug == gym),
        "still drawn on Monday"
    );
    let moved = on(&week, thursday)
        .items
        .iter()
        .find(|i| i.slug == gym)
        .expect("drawn on Thursday");
    assert_eq!(moved.start_ms, at(thursday, 10));
    assert_eq!(moved.moved_from, Some(at(monday, 14)));
    assert_eq!(
        moved.occ,
        Some(at(monday, 14)),
        "named by where the rule put it"
    );

    let next = on(&week, plus(monday, 7));
    assert!(
        next.items
            .iter()
            .any(|i| i.slug == gym && i.moved_from.is_none()),
        "the following Monday is untouched"
    );
}

#[test]
fn a_skipped_occurrence_is_returned_flagged_so_it_can_be_restored() {
    let scratch = Scratch::new("skipped");
    let paths = scratch.paths();
    let monday = a_monday();
    let gym = weekly_gym(&paths, monday);
    mutate::skip_occurrence(&paths, &gym, at(monday, 14)).unwrap();

    let week = read(&paths, monday, monday);
    let skipped = on(&week, monday)
        .items
        .iter()
        .find(|i| i.slug == gym)
        .expect("returned, so a surface has somewhere to put Restore");
    assert!(skipped.skipped);
    assert!(!on(&week, monday).busy(), "a skipped day is not a busy one");

    mutate::restore_occurrence(&paths, &gym, at(monday, 14)).unwrap();
    let week = read(&paths, monday, monday);
    assert!(
        on(&week, monday)
            .items
            .iter()
            .any(|i| i.slug == gym && !i.skipped)
    );
}

#[test]
fn an_untimed_series_asks_its_hour_on_the_calendar_too() {
    let scratch = Scratch::new("unclear");
    let paths = scratch.paths();
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = capture_store::session::capture_body("go to the gym every monday at 2").unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);

    let monday = a_monday();
    let week = read(&paths, monday, plus(monday, 6));
    let gym = on(&week, monday)
        .items
        .iter()
        .find(|i| i.slug == slug)
        .expect("on Monday, without a time");
    assert_eq!(gym.kind, ItemKind::AllDay);
    assert_eq!(
        gym.unclear_hour,
        Some(2),
        "the calendar row is where it is seen"
    );
}

/// "gym every monday at 2pm ~1h" is an hour long every Monday. The `~` was
/// read and dropped for a series, so every session was a mark with no length,
/// while "dentist oct 15 at 2pm ~1h" kept its hour.
#[test]
fn a_written_length_is_kept_by_every_occurrence() {
    let scratch = Scratch::new("length");
    let paths = scratch.paths();
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = capture_store::session::capture_body("gym every monday at 2pm ~1h").unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);

    let monday = a_monday();
    let weeks = read(&paths, monday, plus(monday, 13));
    for d in [monday, plus(monday, 7)] {
        let gym = on(&weeks, d)
            .items
            .iter()
            .find(|i| i.slug == slug)
            .expect("on each Monday");
        assert_eq!(gym.kind, ItemKind::Timed);
        assert_eq!(gym.end_ms, Some(gym.start_ms + HOUR), "on {d}");
    }

    // And none where none was written: an end nobody chose is harder to
    // notice than a missing one.
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = capture_store::session::capture_body("swim every tuesday at 7am").unwrap();
    let swim = s.capture(&body).unwrap().slug;
    drop(s);
    let weeks = read(&paths, monday, plus(monday, 6));
    let swim = on(&weeks, plus(monday, 1))
        .items
        .iter()
        .find(|i| i.slug == swim)
        .expect("on Tuesday");
    assert_eq!(swim.end_ms, None);
}

#[test]
fn a_range_past_the_horizon_is_refused_by_name() {
    let scratch = Scratch::new("horizon");
    let paths = scratch.paths();
    let _ = Session::open(paths.clone()).unwrap();
    let store = capture_store::projection::current(&paths).unwrap();
    let far = plus(capture_time::civil_date(now(), zone()), 400);
    let err = days(&store, far, plus(far, 6), zone(), now()).unwrap_err();
    assert!(err.to_string().contains("year"), "{err}");
}

// ------------------------------------------------- naming and moving by text

fn store_of(paths: &StorePaths) -> capture_store::projection::Store {
    let _ = Session::open(paths.clone()).unwrap();
    capture_store::projection::current(paths).unwrap()
}

fn tgt_of(paths: &StorePaths, slug: &str) -> capture_core::EventId {
    Session::open(paths.clone())
        .unwrap()
        .state
        .resolve(slug)
        .unwrap()
        .unwrap()
}

#[test]
fn an_occurrence_is_named_by_its_date() {
    use capture_store::calendar::occurrence_on;
    let scratch = Scratch::new("named");
    let paths = scratch.paths();
    let monday = a_monday();
    let gym = weekly_gym(&paths, monday);
    let store = store_of(&paths);
    let tgt = tgt_of(&paths, &gym);

    assert_eq!(
        occurrence_on(&store, &tgt, monday, zone()).unwrap(),
        at(monday, 14)
    );

    // A Tuesday is not a day the gym happens on. Refused, naming the Mondays
    // either side so the next attempt is not another guess.
    let err = occurrence_on(&store, &tgt, plus(monday, 1), zone())
        .unwrap_err()
        .to_string();
    assert!(err.contains(&monday.to_string()), "{err}");
    assert!(err.contains(&plus(monday, 7).to_string()), "{err}");
}

#[test]
fn moving_by_text_reads_a_day_and_a_time() {
    use capture_store::calendar::resolve_move;
    let scratch = Scratch::new("totext");
    let paths = scratch.paths();
    let monday = a_monday();
    let gym = weekly_gym(&paths, monday);
    let store = store_of(&paths);
    let tgt = tgt_of(&paths, &gym);
    let occ = at(monday, 14);
    // The Thursday of that Monday's week, not this week's: the Monday is
    // still ahead, so "thursday" counts from it.
    let thursday = plus(monday, 3);

    // "thursday 10am": that moment, and an hour long as the series is.
    let (start, end) = resolve_move(&store, &tgt, occ, "thursday 10am", now(), zone()).unwrap();
    assert_eq!(start, at(thursday, 10));
    assert_eq!(
        end,
        Some(at(thursday, 11)),
        "the hour-long session stays an hour"
    );

    // "thursday": the same slot, on Thursday.
    let (start, _) = resolve_move(&store, &tgt, occ, "thursday", now(), zone()).unwrap();
    assert_eq!(start, at(thursday, 14));

    // A bare hour is asked about here too, not guessed.
    let err = resolve_move(&store, &tgt, occ, "thursday at 2", now(), zone())
        .unwrap_err()
        .to_string();
    assert!(err.contains("2am") && err.contains("2pm"), "{err}");

    // Nothing readable is refused rather than read as now.
    assert!(resolve_move(&store, &tgt, occ, "whenever", now(), zone()).is_err());
}

#[test]
fn moving_by_text_writes_the_move_once() {
    let scratch = Scratch::new("movetext");
    let paths = scratch.paths();
    let monday = a_monday();
    let gym = weekly_gym(&paths, monday);
    let thursday = plus(monday, 3);

    let (start, _) =
        mutate::move_occurrence_to(&paths, &gym, at(monday, 14), "thursday 10am", now(), zone())
            .unwrap();
    assert_eq!(start, at(thursday, 10));

    let days = read(&paths, thursday, thursday);
    let moved = on(&days, thursday)
        .items
        .iter()
        .find(|i| i.slug == gym)
        .expect("on Thursday");
    assert_eq!(moved.moved_from, Some(at(monday, 14)));
}

/// Missed last Monday: "thursday" is the coming Thursday, not one already gone.
#[test]
fn a_past_occurrence_moves_forward_from_now() {
    use capture_store::calendar::resolve_move;
    let scratch = Scratch::new("pastmove");
    let paths = scratch.paths();
    let monday = a_monday();
    let gym = weekly_gym(&paths, monday);
    let store = store_of(&paths);
    let tgt = tgt_of(&paths, &gym);

    let last = capture_time::next_weekday_on_or_after(now() - 7 * 24 * HOUR, zone(), 0).unwrap();
    let last = if at(last, 14) >= now() {
        plus(last, -7)
    } else {
        last
    };
    let coming = capture_time::next_weekday_on_or_after(now(), zone(), 3).unwrap();
    let (start, _) = resolve_move(&store, &tgt, at(last, 14), "thursday", now(), zone()).unwrap();
    assert_eq!(start, at(coming, 14));
}

// ----------------------------------------------------------- grid shapes

#[test]
fn a_month_grid_is_whole_weeks_around_the_month() {
    use capture_store::calendar::{Shape, span};
    let d = |s: &str| FloatingDate::parse(s).unwrap();
    // October 2026 starts on a Thursday and ends on a Saturday.
    let s = span(d("2026-10-12"), Shape::Month, zone()).unwrap();
    assert_eq!((s.from, s.to), (d("2026-09-28"), d("2026-11-01")));
    assert_eq!((s.first, s.last), (d("2026-10-01"), d("2026-10-31")));
    assert_eq!(s.title, "October 2026");
    assert_eq!((s.previous, s.next), (d("2026-09-12"), d("2026-11-12")));

    // The 31st has no 31st a month on; the day clamps rather than spilling.
    let s = span(d("2026-10-31"), Shape::Month, zone()).unwrap();
    assert_eq!(s.next, d("2026-11-30"));
}

#[test]
fn a_week_runs_monday_to_sunday() {
    use capture_store::calendar::{Shape, span, weekday};
    let d = |s: &str| FloatingDate::parse(s).unwrap();
    let s = span(d("2026-10-15"), Shape::Week, zone()).unwrap();
    assert_eq!((s.from, s.to), (d("2026-10-12"), d("2026-10-18")));
    assert_eq!((s.previous, s.next), (d("2026-10-08"), d("2026-10-22")));
    assert_eq!(weekday(d("2026-10-12")), 0, "Monday is zero");
    assert_eq!(weekday(d("2026-10-18")), 6);
}
