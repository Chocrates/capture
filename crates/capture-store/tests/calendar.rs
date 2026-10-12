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
                start_ms: at(plus(monday, -14), 14),
                end_ms: Some(at(plus(monday, -14), 15)),
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
