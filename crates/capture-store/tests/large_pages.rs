//! What a page costs at size (pages-that-hold-up G12).
//!
//! Nothing caps a page, and an agent writing to one will grow it without
//! noticing. So the cost is measured rather than guessed, and a budget well
//! above the measurement turns a regression into a failure.
//!
//! Measured 2026-11-13 on the development laptop, debug build: see the numbers
//! recorded in `openspec/changes/pages-that-hold-up/tasks.md` §5. The budgets
//! below are about twice those, rounded up; a slower machine may need
//! `CAPTURE_SIZE_BUDGET_SCALE`.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "measuring how long work takes, not stamping an event"
)]

use std::time::{Duration, Instant};

use capture_store::session::Session;
use capture_store::{StorePaths, mutate, pages, promote};

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("capture-size-{name}-{}", std::process::id()));
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

fn scale() -> f64 {
    std::env::var("CAPTURE_SIZE_BUDGET_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0)
}

fn timed<T>(what: &str, budget_ms: u64, f: impl FnOnce() -> T) -> T {
    let began = Instant::now();
    let out = f();
    let took = began.elapsed();
    let budget = Duration::from_millis((budget_ms as f64 * scale()) as u64);
    eprintln!(
        "{what}: {} ms (budget {} ms)",
        took.as_millis(),
        budget.as_millis()
    );
    assert!(
        took <= budget,
        "{what} took {took:?}, over its budget of {budget:?}"
    );
    out
}

#[test]
#[ignore = "measures time; run with --ignored, alone (make size)"]
fn a_megabyte_page_and_a_ten_thousand_line_checklist() {
    let scratch = Scratch::new("big");
    let paths = scratch.paths();

    // A megabyte of prose, in lines a person might write.
    let line = "the deploy keeps timing out on staging after the cache warms up again\n";
    let mut big = String::from("Runbook\n");
    while big.len() < 1 << 20 {
        big.push_str(line);
    }
    big.push_str("needle-in-the-haystack\n");
    let runbook = pages::create(&paths, "Runbook", None)
        .unwrap()
        .name
        .unwrap();
    timed("edit a 1 MB page", BUDGET.edit_big, || {
        mutate::edit(&paths, &runbook, &big).unwrap()
    });

    let mut list = String::from("Inventory\n");
    for i in 0..10_000 {
        list.push_str(&format!("- part number {i}\n"));
    }
    let inventory = pages::create(&paths, "Inventory", None)
        .unwrap()
        .name
        .unwrap();
    timed("edit a 10k-line checklist", BUDGET.edit_list, || {
        mutate::edit(&paths, &inventory, &list).unwrap()
    });
    timed("tick one of 10k lines", BUDGET.tick, || {
        promote::tick_at(&paths, &inventory, 9_999, true).unwrap()
    });

    let mut s = Session::open(paths.clone()).unwrap();
    timed("rebuild the projection", BUDGET.rebuild, || {
        s.rebuild_projection().unwrap()
    });
    drop(s);

    let store = capture_store::projection::current(&paths).unwrap();
    let hits = timed("search a needle", BUDGET.search, || {
        store
            .search("needle-in-the-haystack", u64::MAX / 2)
            .unwrap()
    });
    assert_eq!(hits.len(), 1);
    let tgt = store.resolve(&inventory).unwrap().unwrap();
    let items = timed("read 10k checklist lines", BUDGET.items, || {
        store.items_of(&tgt).unwrap()
    });
    assert_eq!(items.len(), 10_000);
    assert!(items[9_999].done, "the tick survived the rebuild");
}

/// Milliseconds. About twice what was measured; see the module comment.
struct Budget {
    edit_big: u64,
    edit_list: u64,
    tick: u64,
    rebuild: u64,
    search: u64,
    items: u64,
}

const BUDGET: Budget = Budget {
    // Measured 2367 / 808 / 854 / 2968 / 1 / 18 ms over three runs.
    edit_big: 5_000,
    edit_list: 2_000,
    tick: 2_000,
    rebuild: 6_000,
    // A millisecond measured; the floor is for timer noise, not the work.
    search: 50,
    items: 50,
};
