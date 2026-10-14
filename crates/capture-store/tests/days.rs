//! A day the words leave open, from capture to an answered day; and the way
//! back from a correction somebody did not mean.
//!
//! Found in use on 2026-10-13: "therapy next thursday at 4pm" landed on the
//! wrong Thursday, the corrected text landed on it again, and accepting the
//! suggestion pinned it there for good. capture-parse tests the reading; these
//! prove the rest — the question is where somebody looks for the appointment,
//! the answer is an ordinary edit, and a correction can be undone.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "the occurrence horizon is measured from the real now"
)]

use capture_core::body::{EventFields, Promoted, When};
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
    std::time::UNIX_EPOCH.elapsed().unwrap().as_millis() as i64
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("capture-days-{name}-{}", std::process::id()));
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

/// A Tuesday at least a week away, so both Thursdays after it are in range.
fn a_tuesday() -> FloatingDate {
    capture_time::next_weekday_on_or_after(now() + 7 * 24 * HOUR, zone(), 1).unwrap()
}

fn plus(d: FloatingDate, n: i64) -> FloatingDate {
    d.plus_days(n).unwrap()
}

/// Captured at noon on `day`, as if written then.
fn capture_on(paths: &StorePaths, day: FloatingDate, text: &str) -> String {
    let noon = capture_time::at_local(day, 12, 0, zone()).unwrap();
    let mut s = Session::open_at(paths.clone(), noon as u64).unwrap();
    let body = capture_store::session::capture_body(text).unwrap();
    s.capture(&body).unwrap().slug
}

fn read(paths: &StorePaths, from: FloatingDate, to: FloatingDate) -> Vec<Day> {
    let _ = Session::open(paths.clone()).unwrap();
    let store = capture_store::projection::current(paths).unwrap();
    days(&store, from, to, zone(), now()).unwrap()
}

/// The days `slug` appears on, and how.
fn appears(days: &[Day], slug: &str) -> Vec<(FloatingDate, ItemKind, Option<[FloatingDate; 2]>)> {
    days.iter()
        .flat_map(|d| {
            d.items
                .iter()
                .filter(|i| i.slug == slug)
                .map(|i| (d.date, i.kind, i.unclear_day))
        })
        .collect()
}

fn text_of(paths: &StorePaths, slug: &str) -> String {
    let store = capture_store::projection::current(paths).unwrap();
    let tgt = store.resolve(slug).unwrap().unwrap();
    store.text_of(&tgt).unwrap().unwrap()
}

fn promotion(paths: &StorePaths, slug: &str) -> Option<capture_store::projection::PromotionRow> {
    let _ = Session::open(paths.clone()).unwrap();
    let store = capture_store::projection::current(paths).unwrap();
    let tgt = store.resolve(slug).unwrap().unwrap();
    store.promotion_of(&tgt).unwrap()
}

#[test]
fn next_thursday_is_asked_on_both_thursdays_and_answered_on_one() {
    let scratch = Scratch::new("next");
    let paths = scratch.paths();
    let tuesday = a_tuesday();
    let slug = capture_on(&paths, tuesday, "therapy next thursday at 4pm");
    let (sooner, later) = (plus(tuesday, 2), plus(tuesday, 9));

    let fortnight = read(&paths, tuesday, plus(tuesday, 13));
    assert_eq!(
        appears(&fortnight, &slug),
        vec![
            (sooner, ItemKind::AllDay, Some([sooner, later])),
            (later, ItemKind::AllDay, Some([sooner, later])),
        ],
        "on both Thursdays, asking, and on no other day"
    );
    assert!(
        fortnight.iter().find(|d| d.date == sooner).unwrap().busy(),
        "a question marks the day"
    );
    assert!(promotion(&paths, &slug).is_none(), "neither day is guessed");

    let settled = mutate::settle_day(&paths, &slug, 1).unwrap();
    assert!(settled.starts_with("therapy thursday "), "{settled}");
    assert!(settled.ends_with(" at 4pm"), "{settled}");
    assert_eq!(text_of(&paths, &slug), settled);

    let fortnight = read(&paths, tuesday, plus(tuesday, 13));
    assert_eq!(
        appears(&fortnight, &slug),
        vec![(later, ItemKind::Timed, None)],
        "on the day chosen, at its time, and asking nothing"
    );
    let at = fortnight
        .iter()
        .find(|d| d.date == later)
        .unwrap()
        .items
        .iter()
        .find(|i| i.slug == slug)
        .unwrap()
        .start_ms;
    assert_eq!(at, capture_time::at_local(later, 16, 0, zone()).unwrap());

    let again = mutate::settle_day(&paths, &slug, 0).unwrap_err();
    assert!(again.to_string().contains("not asking"), "{again}");
}

#[test]
fn a_reading_somebody_pinned_is_not_asked_about() {
    let scratch = Scratch::new("pinned");
    let paths = scratch.paths();
    let tuesday = a_tuesday();
    // A Thursday and the Friday after it: they disagree, whenever written.
    let friday = plus(tuesday, 3);
    let slug = capture_on(
        &paths,
        tuesday,
        &format!("therapy thursday {friday} at 4pm"),
    );
    let week = read(&paths, tuesday, plus(tuesday, 6));
    assert_eq!(
        appears(&week, &slug).len(),
        2,
        "asked before anybody answered"
    );

    promote::correct(&paths, &slug, Promoted::Note).unwrap();
    let week = read(&paths, tuesday, plus(tuesday, 6));
    assert!(appears(&week, &slug).is_empty(), "the pin is the answer");
    assert!(mutate::settle_day(&paths, &slug, 0).is_err());
}

/// The therapy note as it happened: a suggestion accepted that pinned the
/// wrong day, all day, and an edit that could not move it.
#[test]
fn a_correction_read_again_follows_the_text() {
    let scratch = Scratch::new("reread");
    let paths = scratch.paths();
    let tuesday = a_tuesday();
    let thursday = plus(tuesday, 9);
    let slug = capture_on(
        &paths,
        tuesday,
        &format!("therapy thursday {thursday} at 4pm"),
    );
    promote::correct(
        &paths,
        &slug,
        Promoted::Event(EventFields {
            when: When::AllDay {
                start_date: plus(tuesday, 2).to_string(),
                end_date: plus(tuesday, 2).to_string(),
            },
            location: None,
            attendees: Vec::new(),
        }),
    )
    .unwrap();
    assert!(promotion(&paths, &slug).unwrap().pinned);

    assert_eq!(promote::reread(&paths, &slug).unwrap(), "event");
    let p = promotion(&paths, &slug).unwrap();
    assert!(!p.pinned, "no longer a correction");
    let week = read(&paths, tuesday, plus(tuesday, 13));
    assert_eq!(
        appears(&week, &slug),
        vec![(thursday, ItemKind::Timed, None)]
    );

    // And an edit takes again, which is what the pin had stopped.
    let friday = plus(thursday, 1);
    mutate::edit(&paths, &slug, &format!("therapy friday {friday} at 4pm")).unwrap();
    let week = read(&paths, tuesday, plus(tuesday, 13));
    assert_eq!(appears(&week, &slug), vec![(friday, ItemKind::Timed, None)]);

    let err = promote::reread(&paths, &slug).unwrap_err();
    assert!(err.to_string().contains("nothing to undo"), "{err}");
}

#[test]
fn a_suggestion_says_what_accepting_it_would_leave_out() {
    let thursday = capture_store::review::suggestion_for(
        "dentist thursday, maybe 4pm",
        Some(ZONE),
        capture_time::at_local(a_tuesday(), 12, 0, zone()).unwrap(),
    )
    .expect("a bare day is a suggestion");
    assert!(thursday.contains("all day"), "{thursday}");
    assert!(
        thursday.contains("Thu "),
        "the weekday is named: {thursday}"
    );
    assert!(
        thursday.contains("4pm"),
        "the time left out is named: {thursday}"
    );
}
