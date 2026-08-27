//! Recurring obligations, and why missing one leaves nothing behind.
//!
//! §3.6's departure from how nearly every todo app behaves, and the reasoning
//! is not a preference. The habit literature is explicit that a single missed
//! day costs almost nothing — under half a point of automaticity, recovered
//! quickly — so a system that manufactures a backlog is representing something
//! that is *not true* about how habits form. And `EVIDENCE.md` §3 reports
//! anxiety in ADHD as frequently secondary to repeated performance failures,
//! which makes a visible pile of them a mechanism-level concern rather than a
//! matter of taste.
//!
//! The exception had to exist, and these check both halves.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "comparing against timestamps the store already wrote"
)]

use brain_core::body::{Promoted, RuleSpec, SeriesFields, When};
use brain_store::session::Session;
use brain_store::{StorePaths, promote};

const DAY: i64 = 24 * 60 * 60 * 1000;

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("brain-lapse-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        brain_store::store::create(&StorePaths::under(p.clone()), 1_700_000_000_000).unwrap();
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

/// A daily obligation that started a week ago and has been missed throughout.
fn daily(paths: &StorePaths, text: &str, carries_forward: bool) -> brain_core::EventId {
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = brain_store::session::capture_body(text).unwrap();
    let slug = s.capture(&body).unwrap().slug;
    drop(s);

    promote::correct(
        paths,
        &slug,
        Promoted::Series(SeriesFields {
            when: When::Timed {
                start_ms: now() - 7 * DAY,
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
            carries_forward,
            split_from: None,
        }),
    )
    .unwrap();

    let s = Session::open_for_capture(paths.clone()).unwrap();
    s.state.resolve(&slug).unwrap().unwrap()
}

fn store(paths: &StorePaths) -> brain_store::projection::Store {
    let generation = brain_store::ops::Ops::open_read(&paths.ops)
        .unwrap()
        .generation()
        .unwrap();
    brain_store::projection::Store::open_read(&brain_store::projection::generation_path(
        &paths.state,
        generation,
    ))
    .unwrap()
}

#[test]
fn a_missed_instance_lapses_and_is_recorded() {
    // Recorded as lapsed, which is not the same as forgotten. "Nothing
    // accumulates" is a statement about debt, not about memory.
    let sc = Scratch::new("recorded");
    let p = sc.paths();
    let series = daily(&p, "walk the dog every day", false);

    let lapsed = store(&p).lapsed(&series, now()).unwrap();
    assert!(lapsed.len() >= 6, "a week of misses, got {}", lapsed.len());
}

#[test]
fn nothing_is_owed_by_default() {
    // The whole of §3.6. Miss Tuesday's walk and there is no debt on Wednesday.
    let sc = Scratch::new("no-debt");
    let p = sc.paths();
    daily(&p, "walk the dog every day", false);

    assert!(
        store(&p).outstanding(now()).unwrap().is_empty(),
        "a series that does not carry forward produced a backlog"
    );
}

#[test]
fn a_day_shows_one_instance_however_many_were_missed() {
    // The visible half of not stacking: a week of misses does not pile up onto
    // today. This falls out of occurrences being per-window rather than being a
    // queue, which is worth pinning — it is the sort of property a later
    // "helpful" change would quietly break.
    let sc = Scratch::new("no-stack");
    let p = sc.paths();
    daily(&p, "walk the dog every day", false);

    let s = store(&p);
    let start_of_today = now() - (now() % DAY);
    let today = s
        .happening_between(start_of_today, start_of_today + DAY)
        .unwrap();

    assert_eq!(today.len(), 1, "a week of misses stacked onto one day");
}

#[test]
fn a_dose_is_still_owed() {
    // The exception that had to exist. For a missed dose or an unpaid invoice,
    // Wednesday carries a real debt, and silently dropping it is worse than the
    // todo-app failure the default was written to fix — especially for somebody
    // whose presenting difficulty is forgetting things.
    let sc = Scratch::new("owed");
    let p = sc.paths();
    daily(&p, "take the pill every day", true);

    let owed = store(&p).outstanding(now()).unwrap();
    assert!(owed.len() >= 6, "a carried-forward series owes nothing");
}

#[test]
fn completing_an_instance_settles_it() {
    let sc = Scratch::new("settled");
    let p = sc.paths();
    let series = daily(&p, "take the pill every day", true);

    let first = store(&p).lapsed(&series, now()).unwrap()[0].occ;
    let before = store(&p).outstanding(now()).unwrap().len();

    let mut s = Session::open(p.clone()).unwrap();
    s.append(
        brain_core::event::Kind::OCC_DONE,
        Some(series),
        None,
        |nonce| {
            let mut b = Vec::new();
            minicbor::encode(&brain_core::body::OccBody { nonce, occ: first }, &mut b).unwrap();
            b
        },
    )
    .unwrap();
    drop(s);

    assert_eq!(store(&p).outstanding(now()).unwrap().len(), before - 1);
}

#[test]
fn a_repeatedly_lapsing_series_is_worth_asking_about() {
    // Asking once, rather than inferring. §3.6: the recognizer never sets
    // `carries_forward`, because "walk the dog daily" and "take the pill daily"
    // are the same sentence to a parser and opposite answers to a person.
    let sc = Scratch::new("ask");
    let p = sc.paths();
    let series = daily(&p, "walk the dog every day", false);

    let asked = store(&p).repeatedly_lapsed(now(), 3).unwrap();
    assert!(asked.contains(&series), "nothing noticed a week of misses");
}

#[test]
fn a_series_that_already_carries_forward_is_not_asked_about() {
    // It has been answered. Asking again is nagging, which is the thing §2.7
    // and §3.6 both spend their length avoiding.
    let sc = Scratch::new("answered");
    let p = sc.paths();
    let series = daily(&p, "take the pill every day", true);

    assert!(
        !store(&p)
            .repeatedly_lapsed(now(), 3)
            .unwrap()
            .contains(&series)
    );
}
