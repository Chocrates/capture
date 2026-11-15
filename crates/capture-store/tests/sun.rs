//! Times set by the sun, in the store (alarms-at-sunrise-and-sunset D1–D4, D7).
//!
//! An occurrence is named by its day; where the phone is decides only when it
//! rings. Where the phone is lives in `ops.db` and never in the log.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "a series expands around the real now, so the tests do too"
)]

use capture_core::body::{EventFields, Promoted, RuleSpec, SUN_RISE, SUN_SET, SeriesFields, When};
use capture_store::here::{self, Here};
use capture_store::session::Session;
use capture_store::{StorePaths, mutate, promote};
use capture_time::{SunEvent, Zone};
use proptest::test_runner::{Config, TestRunner};

const LONDON: (f64, f64) = (51.4823, -0.0123);
const SVALBARD: (f64, f64) = (78.22, 15.65);
const DAY: i64 = 86_400_000;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("capture-sun-{name}-{}", std::process::id()));
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

mod common;
use common::now;

fn london() -> Zone {
    Zone::new("Europe/London").unwrap()
}

fn at(p: (f64, f64)) -> Here {
    Here::from_degrees(p.0, p.1, now()).unwrap()
}

/// A note promoted by hand to `as_`, and its id.
fn promoted(paths: &StorePaths, text: &str, as_: Promoted) -> (String, capture_core::EventId) {
    let mut s = Session::open(paths.clone()).unwrap();
    let body = capture_store::session::capture_body_in(text, Some("Europe/London")).unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);
    promote::correct(paths, &slug, as_).unwrap();
    let s = Session::open(paths.clone()).unwrap();
    let tgt = s.state.resolve(&slug).unwrap().unwrap();
    (slug, tgt)
}

/// Every day, 30 minutes before sunrise, from today in London.
fn daily_dawn(paths: &StorePaths) -> (String, capture_core::EventId) {
    let today = capture_time::civil_date(now(), london());
    promoted(
        paths,
        "kettle on",
        Promoted::Series(SeriesFields {
            when: When::Solar {
                date: format!("{today}"),
                event: SUN_RISE,
                offset_min: -30,
                tz: "Europe/London".into(),
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
}

fn store(paths: &StorePaths) -> capture_store::projection::Store {
    capture_store::projection::current(paths).unwrap()
}

#[test]
fn a_sun_series_is_named_by_its_days_not_by_where_you_are() {
    let sc = Scratch::new("named");
    let p = sc.paths();
    let (_, tgt) = daily_dawn(&p);
    let st = store(&p);

    let occs = st.occurrences_of(&tgt).unwrap();
    assert!(occs.len() > 300, "a daily series expands a year ahead");
    for o in &occs {
        let date = capture_time::civil_date(o.occ, london());
        assert_eq!(
            Some(o.occ),
            capture_time::at_local(date, 0, 0, london()),
            "an occurrence is named by its day's local midnight"
        );
        assert!(o.all_day, "a sun occurrence has no stored clock");
    }
    let ids: std::collections::BTreeSet<i64> = occs.iter().map(|o| o.occ).collect();

    // For every place the sun rises and sets each day, the same days come due
    // under the same names, each at that day's sunrise there less 30 minutes.
    let n = now();
    let horizon = n + 4 * DAY;
    let mut runner = TestRunner::new(Config::with_cases(256));
    runner
        .run(&(-6_000i32..=6_000, -18_000i32..=18_000), |(lat, lon)| {
            let here = Here {
                lat_e6: lat * 100,
                lon_e6: lon * 100,
                fix_ms: n,
            };
            let due: Vec<_> = st
                .due_by_at(horizon, n, "Europe/London", Some(here))
                .unwrap()
                .into_iter()
                .filter(|d| d.tgt == tgt)
                .collect();
            for d in &due {
                let occ = d.occ.expect("a series moment names its occurrence");
                assert!(
                    ids.contains(&occ),
                    "{occ} is not an occurrence of the series"
                );
                let date = capture_time::civil_date(occ, london());
                let rise =
                    capture_time::sun(date, london(), here.lat_e6, here.lon_e6, SunEvent::Rise)
                        .unwrap();
                assert_eq!(d.at, rise - 30 * 60_000);
                assert!(d.timed, "a sun time is a time of day (D7)");
                assert!(d.at <= horizon);
            }
            // Every day wholly inside the horizon is there, wherever this is.
            let inside = ids.iter().filter(|o| **o + 2 * DAY <= horizon).count();
            assert!(
                due.len() >= inside,
                "{lat} {lon}: {} of {inside}",
                due.len()
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn nowhere_known_nothing_rings() {
    let sc = Scratch::new("nowhere");
    let p = sc.paths();
    let (_, tgt) = daily_dawn(&p);
    let st = store(&p);
    let n = now();
    assert!(
        st.due_by_at(n + 3 * DAY, n, "Europe/London", None)
            .unwrap()
            .iter()
            .all(|d| d.tgt != tgt),
        "with no place, a sun time was guessed"
    );
    assert!(
        st.announce_by_at(n + 3 * DAY, n, "Europe/London", 15 * 60_000, None)
            .unwrap()
            .iter()
            .all(|d| d.tgt != tgt)
    );
    assert!(
        st.due_by_at(n + 3 * DAY, n, "Europe/London", Some(at(LONDON)))
            .unwrap()
            .iter()
            .any(|d| d.tgt == tgt),
        "and with one, it is due"
    );
}

#[test]
fn no_sunrise_rings_nothing() {
    let sc = Scratch::new("polar");
    let p = sc.paths();
    let (_, tgt) = promoted(
        &p,
        "photos",
        Promoted::Event(EventFields {
            when: When::Solar {
                date: "2026-12-21".into(),
                event: SUN_RISE,
                offset_min: 0,
                tz: "Europe/London".into(),
            },
            location: None,
            attendees: vec![],
        }),
    );
    let st = store(&p);
    let horizon = capture_time::at_local(
        capture_time::FloatingDate::parse("2026-12-23").unwrap(),
        0,
        0,
        london(),
    )
    .unwrap();
    let n = now();
    let due = |here| -> Vec<_> {
        st.due_by_at(horizon, n, "Europe/London", Some(here))
            .unwrap()
            .into_iter()
            .filter(|d| d.tgt == tgt)
            .collect()
    };
    assert!(due(at(SVALBARD)).is_empty(), "polar night rang a sunrise");
    let there = due(at(LONDON));
    assert_eq!(there.len(), 1, "a one-off sun time is one moment");
    let h = at(LONDON);
    let date = capture_time::FloatingDate::parse("2026-12-21").unwrap();
    assert_eq!(
        there[0].at,
        capture_time::sun(date, london(), h.lat_e6, h.lon_e6, SunEvent::Rise).unwrap()
    );
}

#[test]
fn done_and_moved_are_kept_by_the_days_name() {
    let sc = Scratch::new("exceptions");
    let p = sc.paths();
    let (slug, tgt) = daily_dawn(&p);
    let occs: Vec<i64> = store(&p)
        .occurrences_of(&tgt)
        .unwrap()
        .iter()
        .map(|o| o.occ)
        .collect();
    let (first, second) = (occs[0], occs[1]);

    mutate::set_occurrence_done(&p, &slug, first, true).unwrap();
    // Moved to 06:15 on its own day: a fixed instant now, wherever this is.
    let fixed = second + (6 * 60 + 15) * 60_000;
    mutate::move_occurrence(&p, &slug, second, fixed, None).unwrap();

    let st = store(&p);
    let n = now();
    for here in [None, Some(at(LONDON)), Some(at((-33.87, 151.21)))] {
        let due: Vec<_> = st
            .due_by_at(n + 3 * DAY, n, "Europe/London", here)
            .unwrap()
            .into_iter()
            .filter(|d| d.tgt == tgt)
            .collect();
        assert!(
            due.iter().all(|d| d.occ != Some(first)),
            "a done day came due again at {here:?}"
        );
        let moved: Vec<_> = due.iter().filter(|d| d.occ == Some(second)).collect();
        assert_eq!(moved.len(), 1, "{here:?}");
        assert_eq!(
            moved[0].at, fixed,
            "a moved day follows the move, not the sun"
        );
        assert!(moved[0].timed);
    }
    // And the names the marks were written against are still the names.
    let after: Vec<i64> = st
        .occurrences_of(&tgt)
        .unwrap()
        .iter()
        .map(|o| o.occ)
        .collect();
    assert_eq!(after[0], first);
    assert!(after.contains(&second));
}

#[test]
fn here_is_rounded_and_only_moves_forward() {
    let sc = Scratch::new("here");
    let p = sc.paths();
    let ops = capture_store::ops::Ops::open(&p.ops).unwrap();
    assert_eq!(here::get(&ops).unwrap(), None);

    let h = Here::from_degrees(LONDON.0, LONDON.1, 2_000).unwrap();
    assert_eq!((h.lat_e6, h.lon_e6), (51_480_000, -10_000));
    here::set(&ops, h).unwrap();
    assert_eq!(here::get(&ops).unwrap(), Some(h));

    // An older fix arriving late does not replace a newer one.
    here::set(&ops, Here::from_degrees(-33.87, 151.21, 1_000).unwrap()).unwrap();
    assert_eq!(here::get(&ops).unwrap(), Some(h));

    for (lat, lon) in [(91.0, 0.0), (0.0, 181.0), (f64::NAN, 0.0)] {
        assert!(Here::from_degrees(lat, lon, 3_000).is_err());
    }
    assert!(h.is_stale(2_000 + here::STALE_MS + 1));
    assert!(!h.is_stale(2_000 + here::STALE_MS));
}

#[test]
fn where_you_are_never_reaches_the_log() {
    let sc = Scratch::new("never-logged");
    let p = sc.paths();
    {
        let ops = capture_store::ops::Ops::open(&p.ops).unwrap();
        here::set(&ops, at(LONDON)).unwrap();
    }
    // Everything that writes to the log while a place is known: a capture the
    // recognizer reads as a sun time, a hand promotion, a done mark.
    let mut s = Session::open(p.clone()).unwrap();
    let body = capture_store::session::capture_body_in(
        "swim every day 20 min after sunset",
        Some("Europe/London"),
    )
    .unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);
    let (dawn, tgt) = daily_dawn(&p);
    let first = store(&p).occurrences_of(&tgt).unwrap()[0].occ;
    mutate::set_occurrence_done(&p, &dawn, first, true).unwrap();
    drop(Session::open(p.clone()).unwrap());

    let st = store(&p);
    let swim = st.resolve(&slug).unwrap().unwrap();
    assert!(
        !st.occurrences_of(&swim).unwrap().is_empty(),
        "the capture was read as a sun series, or this proves nothing"
    );

    // The log is the only thing sync sends (§2.5.2), so what is not in it is
    // not on the wire either.
    let mut log = Vec::new();
    for e in walk(&p.events) {
        log.extend(std::fs::read(e).unwrap());
    }
    assert!(!log.is_empty());
    let h = at(LONDON);
    let mut needles: Vec<Vec<u8>> = vec![
        b"51.48".to_vec(),
        b"51480000".to_vec(),
        h.lat_e6.to_le_bytes().to_vec(),
        h.lat_e6.to_be_bytes().to_vec(),
        minicbor::to_vec(h.lat_e6).unwrap(),
        minicbor::to_vec(5148i32).unwrap(),
        minicbor::to_vec(51.48f64).unwrap(),
    ];
    needles.retain(|n| n.len() >= 3);
    for n in &needles {
        assert!(
            !log.windows(n.len()).any(|w| w == n.as_slice()),
            "the log holds {n:02x?}, which is where the phone was"
        );
    }
}

fn walk(p: &std::path::Path) -> Vec<std::path::PathBuf> {
    if p.is_file() {
        return vec![p.to_path_buf()];
    }
    let mut out = Vec::new();
    for e in std::fs::read_dir(p).unwrap() {
        out.extend(walk(&e.unwrap().path()));
    }
    out
}

#[test]
fn a_sunset_series_starts_the_day_it_was_written() {
    // The recognizer's reading, through the promoter: no instant, no place.
    let sc = Scratch::new("read");
    let p = sc.paths();
    let mut s = Session::open(p.clone()).unwrap();
    let body =
        capture_store::session::capture_body_in("lamps every day at sunset", Some("Europe/London"))
            .unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);
    // Read on the next open, as the sweep reads every capture.
    let s = Session::open(p.clone()).unwrap();
    let tgt = s.state.resolve(&slug).unwrap().unwrap();
    let all = s.log.all().unwrap();
    let folded = capture_core::fold::fold(&all);
    let promo = folded.notes[&tgt].promotion.clone().expect("promoted");
    match promo.as_ {
        Promoted::Series(SeriesFields {
            when:
                When::Solar {
                    date,
                    event,
                    offset_min,
                    tz,
                },
            ..
        }) => {
            assert_eq!(
                date,
                format!("{}", capture_time::civil_date(now(), london()))
            );
            assert_eq!(
                (event, offset_min, tz.as_str()),
                (SUN_SET, 0, "Europe/London")
            );
        }
        other => panic!("promoted as {other:?}"),
    }
}

#[test]
fn notes_an_older_grammar_read_are_reported_when_they_become_sun_times() {
    let sc = Scratch::new("reread");
    let p = sc.paths();
    let ops = || capture_store::ops::Ops::open_read(&p.ops).unwrap();

    // A capture read fresh as a sun time is not a backlog surprise.
    {
        let mut s = Session::open(p.clone()).unwrap();
        let body =
            capture_store::session::capture_body_in("swim at sunrise", Some("Europe/London"))
                .unwrap();
        s.capture(&body).unwrap();
    }
    promote::settle(&p).unwrap();
    assert_eq!(promote::sun_reread(&ops()).unwrap(), None);

    // Three that grammar 9 read and found nothing in, or a day: the next open
    // reads them again under 10.
    let mut slugs = Vec::new();
    {
        let mut s = Session::open(p.clone()).unwrap();
        for text in [
            "drinks at sunset",
            "dentist thursday 2pm",
            "watched the sunset",
        ] {
            let body =
                capture_store::session::capture_body_in(text, Some("Europe/London")).unwrap();
            let slug = s.capture(&body).unwrap().slug;
            let tgt = s.state.resolve(&slug).unwrap().unwrap();
            let note = s.state.note(&tgt).unwrap().unwrap();
            s.state
                .mark_read(&tgt, capture_parse::GRAMMAR_VERSION - 1, note.updated)
                .unwrap();
            slugs.push(slug);
        }
    }
    promote::settle(&p).unwrap();
    let (at, listed) = promote::sun_reread(&ops()).unwrap().expect("reported");
    assert_eq!(
        listed,
        vec![slugs[0].clone()],
        "only the one that became a sun time"
    );
    assert!((now() - at).abs() < 60_000);
}

#[test]
fn a_sun_day_is_drawn_once_and_says_what_it_hangs_from() {
    let sc = Scratch::new("drawn");
    let p = sc.paths();
    let (_, once) = promoted(
        &p,
        "lanterns",
        Promoted::Event(EventFields {
            when: When::Solar {
                date: "2026-12-21".into(),
                event: SUN_SET,
                offset_min: -30,
                tz: "Europe/London".into(),
            },
            location: None,
            attendees: vec![],
        }),
    );
    let st = store(&p);
    let day = capture_time::FloatingDate::parse("2026-12-21").unwrap();
    let days = capture_store::calendar::days(&st, day, day, london(), now()).unwrap();
    let mine: Vec<_> = days[0].items.iter().filter(|i| i.tgt == once).collect();
    assert_eq!(
        mine.len(),
        1,
        "a one-off sun day was drawn {} times",
        mine.len()
    );
    assert_eq!(mine[0].sun.as_deref(), Some("30 min before sunset"));
}

#[test]
fn a_split_sun_series_starts_where_it_was_split() {
    let sc = Scratch::new("split");
    let p = sc.paths();
    let (slug, tgt) = daily_dawn(&p);
    let occs: Vec<i64> = store(&p)
        .occurrences_of(&tgt)
        .unwrap()
        .iter()
        .map(|o| o.occ)
        .collect();
    let boundary = occs[10];
    let rule = RuleSpec {
        freq: 1,
        interval: 1,
        by_day: 0b0001_1111,
        by_month_day: Vec::new(),
        count: None,
        until: None,
    };
    promote::split_series(&p, &slug, boundary, rule).unwrap();

    let s = Session::open(p.clone()).unwrap();
    let folded = capture_core::fold::fold(&s.log.all().unwrap());
    let split = folded
        .notes
        .values()
        .filter_map(|n| match n.promotion.as_ref().map(|p| &p.as_) {
            Some(Promoted::Series(sf)) if sf.split_from == Some(tgt) => Some(sf.when.clone()),
            _ => None,
        })
        .next()
        .expect("a new series");
    assert_eq!(
        split,
        When::Solar {
            date: format!("{}", capture_time::civil_date(boundary, london())),
            event: SUN_RISE,
            offset_min: -30,
            tz: "Europe/London".into(),
        },
        "the new series kept the sun and starts at the split"
    );
}
