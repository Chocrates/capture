//! What the expansion horizon actually costs.
//!
//! Task 6.1a. The ±1 year window was kept rather than re-argued, on the
//! understanding that it would be measured once instead of guessed at twice.
//! Ignored by default: it is a measurement, not an assertion, and a number that
//! fails CI on a slow machine would tell nobody anything.
//!
//! Run it with `cargo test -p capture-store --test horizon_cost -- --ignored
//! --nocapture`.

// Timing a rebuild, which is the entire point of the file. The crate-wide ban
// exists so the fold cannot consult a clock; this is a stopwatch on a
// measurement whose reading never reaches an event.
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "measuring how long a rebuild takes, not stamping an event"
)]

use capture_core::body::{Promoted, RuleSpec, SeriesFields, When};
use capture_store::session::Session;
use capture_store::{StorePaths, promote};

const NOW: i64 = 1_780_326_000_000;

#[test]
#[ignore = "a measurement, not an assertion"]
fn a_dozen_daily_series() {
    let dir = std::env::temp_dir().join(format!("capture-horizon-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let paths = StorePaths::under(dir.clone());
    capture_store::store::create(&paths, 1_700_000_000_000).unwrap();

    for i in 0..12 {
        let mut s = Session::open(paths.clone()).unwrap();
        let body =
            capture_store::session::capture_body(&format!("habit number {i} every day")).unwrap();
        let slug = s.capture(&body).unwrap().slug;
        drop(s);

        promote::correct(
            &paths,
            &slug,
            Promoted::Series(SeriesFields {
                when: When::Timed {
                    start_ms: NOW,
                    end_ms: None,
                    tz: "America/Denver".into(),
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
        .unwrap();
    }

    let s = Session::open(paths.clone()).unwrap();
    let all = s.log.all().unwrap();
    drop(s);

    let started = std::time::Instant::now();
    let next = capture_store::projection::rebuild(&paths.state, &all, &[], 0, NOW).unwrap();
    let elapsed = started.elapsed();

    let store = capture_store::projection::Store::open_read(
        &capture_store::projection::generation_path(&paths.state, next),
    )
    .unwrap();

    let rows = store
        .occurring_between(NOW - 400 * 86_400_000, NOW + 400 * 86_400_000)
        .unwrap()
        .len();

    let db = capture_store::projection::generation_path(&paths.state, next);
    let bytes = std::fs::metadata(&db).map(|m| m.len()).unwrap_or(0);

    println!("--- horizon cost: 12 unbounded daily series, ±1 year ---");
    println!("occurrence rows : {rows}");
    println!("rebuild         : {elapsed:?}");
    println!("projection size : {} KiB", bytes / 1024);

    let _ = std::fs::remove_dir_all(&dir);
}
