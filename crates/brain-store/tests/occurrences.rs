//! Occurrence expansion, from a capture to rows a calendar can draw.
//!
//! The unit tests in `brain-time` prove the expansion itself is right. These
//! prove the projection stores what it produced, applies the exceptions on top,
//! and throws all of it away and recomputes it on a rebuild — which is what
//! makes occurrences derived data rather than a second truth (§3.4).

use brain_core::body::{Promoted, RuleSpec, SeriesFields, When};
use brain_core::event::Kind;
use brain_store::session::Session;
use brain_store::{StorePaths, promote};

/// Monday 1 June 2026, 09:00 in Denver.
const NOW: i64 = 1_780_326_000_000;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("brain-occ-{name}-{}", std::process::id()));
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

/// A capture promoted by hand to a weekday series at 07:00.
fn weekday_series(paths: &StorePaths) -> (String, brain_core::EventId) {
    let mut s = Session::open(paths.clone()).unwrap();
    let body = brain_store::session::capture_body("walk the dog every weekday at 7").unwrap();
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
        }),
    )
    .unwrap();

    let s = Session::open(paths.clone()).unwrap();
    let tgt = s.state.resolve(&slug).unwrap().unwrap();
    (slug, tgt)
}

fn rebuild(paths: &StorePaths, now: i64) -> brain_store::projection::Store {
    let s = Session::open(paths.clone()).unwrap();
    let all = s.log.all().unwrap();
    let generation = brain_store::ops::Ops::open(&paths.ops)
        .unwrap()
        .generation()
        .unwrap();
    let next = brain_store::projection::rebuild(&paths.state, &all, generation, now).unwrap();
    brain_store::ops::Ops::open(&paths.ops)
        .unwrap()
        .set_generation(next)
        .unwrap();
    drop(s);
    brain_store::projection::Store::open_read(&brain_store::projection::generation_path(
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
        minicbor::encode(&brain_core::body::OccBody { nonce, occ: target }, &mut b).unwrap();
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
            &brain_core::body::OccMoveBody {
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
