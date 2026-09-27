//! Banding open tasks into overdue, due and undated.
//!
//! Property-tested rather than example-tested, because the thing most likely to
//! be wrong is a boundary: a task due *today* is not overdue, and a handful of
//! hand-picked dates will agree with an off-by-one as readily as with the fix.
//! The properties below hold for any set of tasks and any day.

use capture_store::projection::Band;
use capture_store::session::Session;
use capture_store::{StorePaths, promote};
use proptest::prelude::*;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "capture-outstanding-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
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

/// Catch the projection up, then read it. A read alone would see a store that
/// has not folded the writes this test just made.
fn settled(paths: &StorePaths) -> capture_store::projection::Store {
    let _ = Session::open(paths.clone()).unwrap();
    capture_store::projection::current(paths).unwrap()
}

fn day(n: u32) -> String {
    format!("2026-03-{n:02}")
}

/// One task as the test asks for it: a due day, or none, and whether it is done.
#[derive(Clone, Debug)]
struct Wanted {
    due: Option<u32>,
    done: bool,
}

fn wanted() -> impl Strategy<Value = Wanted> {
    (proptest::option::of(1u32..=28), any::<bool>()).prop_map(|(due, done)| Wanted { due, done })
}

proptest! {
    // A store per case is real I/O, so the case count is deliberately low. The
    // point is covering the boundary, not the volume.
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn every_open_task_lands_in_exactly_one_right_band(
        tasks in prop::collection::vec(wanted(), 0..8),
        today in 1u32..=28,
    ) {
        let scratch = Scratch::new("bands");
        let paths = scratch.paths();

        for (i, w) in tasks.iter().enumerate() {
            let slug = capture(&paths, &format!("task number {i}"));
            promote::as_task(&paths, &slug, w.due.map(day).as_deref()).unwrap();
            if w.done {
                promote::set_done(&paths, &slug, true).unwrap();
            }
        }

        let rows = settled(&paths).tasks_outstanding(&day(today)).unwrap();

        // Done tasks are absent entirely: a surface asking what is outstanding
        // is not asking what was.
        let open = tasks.iter().filter(|w| !w.done).count();
        prop_assert_eq!(rows.len(), open, "a done task reached the bands");

        for row in &rows {
            match row.due.as_deref() {
                // Undated is the ordinary case, and it is never anything else.
                None => prop_assert_eq!(row.band, Band::Undated),
                Some(d) => {
                    let expected = if d < day(today).as_str() {
                        Band::Overdue
                    } else {
                        // Due *today* is due, not overdue. This is the boundary.
                        Band::Due
                    };
                    prop_assert_eq!(row.band, expected, "{} against today {}", d, day(today));
                }
            }
        }

        // Dated first in due order, undated after. An undated task appearing
        // between two dated ones would mean the ordering does not hold.
        let first_undated = rows.iter().position(|r| r.due.is_none());
        if let Some(at) = first_undated {
            prop_assert!(
                rows[at..].iter().all(|r| r.due.is_none()),
                "a dated task sorted after an undated one"
            );
        }
    }
}

#[test]
fn an_undated_task_is_outstanding_and_is_not_on_the_agenda() {
    // The whole reason this query exists. A task occupies no time (§3.6), so
    // filing an undated one onto a day would be the helpful-looking change that
    // puts it back where it could not be found.
    let scratch = Scratch::new("agenda");
    let paths = scratch.paths();

    let slug = capture(&paths, "send sarah the migration doc");
    promote::as_task(&paths, &slug, None).unwrap();

    let store = settled(&paths);
    let today = day(10);

    let rows = store.tasks_outstanding(&today).unwrap();
    assert_eq!(rows.len(), 1, "the undated task is outstanding");
    assert_eq!(rows[0].band, Band::Undated);
    assert!(rows[0].due.is_none());

    // Nothing reaches a day, and nothing reaches the overdue arm either —
    // "no due date" is not "a due date in the past".
    assert!(
        store.tasks_due_on(&today).unwrap().is_empty(),
        "an undated task was filed onto a day"
    );
    assert!(
        store.tasks_overdue(&today).unwrap().is_empty(),
        "an undated task was treated as overdue"
    );
}

#[test]
fn a_finished_task_leaves_the_bands() {
    let scratch = Scratch::new("done");
    let paths = scratch.paths();

    let slug = capture(&paths, "call the dentist");
    promote::as_task(&paths, &slug, Some(&day(5))).unwrap();

    let today = day(10);
    assert_eq!(
        settled(&paths).tasks_outstanding(&today).unwrap().len(),
        1,
        "an overdue task is outstanding before it is done"
    );

    promote::set_done(&paths, &slug, true).unwrap();
    assert!(
        settled(&paths)
            .tasks_outstanding(&today)
            .unwrap()
            .is_empty(),
        "a finished task stayed outstanding"
    );
}
