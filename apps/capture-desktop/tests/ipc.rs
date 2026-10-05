//! The window's commands, asked directly.
//!
//! # One test, on purpose
//!
//! Every command finds its store through `CAPTURE_HOME` and its zone through
//! `TZ`, and both are process-global. Two `#[test]` functions pointing them at
//! different places would race, and the failure would look like a bug in the
//! thing under test rather than in the harness. So this is one test that walks
//! through a store, and the sections are labelled.
//!
//! # Why this file exists at all
//!
//! Until the crate had a library target, nothing could call these. The only
//! test over this crate reads `app.js` as *text*, which can check that the page
//! names a command and cannot check what the command does.
//!
//! That is not hypothetical. `ipc::agenda` built its day window in UTC, so
//! every evening appointment west of the meridian landed on the wrong day
//! while the terminal showed it correctly — a four-line mistake over a correct
//! store query, found by a person comparing two surfaces. The first assertion
//! below is the one that would have caught it.

use capture_desktop::ipc;

/// A scratch store, and the environment pointed at it.
///
/// `set_var` is unsafe in this edition because another thread may be reading
/// the environment. There is one thread here — that is what the single test
/// buys — and the alternative is spawning a process per assertion, which is
/// how this surface came to be untested in the first place.
fn point_at(dir: &std::path::Path, zone: &str) {
    unsafe {
        std::env::set_var("CAPTURE_HOME", dir);
        std::env::set_var("TZ", zone);
    }
}

#[test]
fn the_window_answers_what_the_store_holds() {
    let dir = std::env::temp_dir().join(format!("capture-ipc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    point_at(&dir, "America/Denver");

    let paths = capture_store::StorePaths::under(dir.clone());
    capture_store::store::create(&paths, 1_700_000_000_000).unwrap();

    // ---------------------------------------------------------------- agenda
    //
    // A day is the day the reader is in. 19:00 in Denver on the 5th is 01:00
    // UTC on the 6th, so a window built in UTC puts this on the wrong day —
    // which is exactly what it did.
    capture(&paths, "dinner 2026-10-05 19:00");
    capture_store::promote::settle(&paths).unwrap();

    let fifth = ipc::agenda("2026-10-05".into()).expect("the agenda answers");
    assert!(
        fifth.iter().any(|o| o.slug.starts_with("dinner")),
        "an evening appointment is missing from its own day: {fifth:?}"
    );
    let sixth = ipc::agenda("2026-10-06".into()).expect("the agenda answers");
    assert!(
        !sixth.iter().any(|o| o.slug.starts_with("dinner")),
        "an evening appointment appeared on the following day too: {sixth:?}"
    );

    // ----------------------------------------------------------------- tasks
    //
    // The three bands, with the undated one present: a task occupies no time,
    // so the agenda cannot hold it, and an undated task reached no window
    // surface at all until this command existed.
    let undated = capture(&paths, "send sarah the migration doc");
    capture_store::promote::as_task(&paths, &undated, None).unwrap();
    let overdue = capture(&paths, "pay the invoice");
    capture_store::promote::as_task(&paths, &overdue, Some("2020-01-01")).unwrap();

    let tasks = ipc::tasks().expect("the tasks answer");
    let band = |slug: &str| {
        tasks
            .iter()
            .find(|t| t.slug == slug)
            .map(|t| t.band)
            .unwrap_or("missing")
    };
    assert_eq!(band(&undated), "undated", "{tasks:?}");
    assert_eq!(band(&overdue), "overdue", "{tasks:?}");

    // ------------------------------------------------------------------ list
    //
    // Every line says which capture it came from. Without that the window can
    // draw a gathered list and tick nothing on it, because position 0 exists in
    // every capture under the tag.
    let shop_a = capture(&paths, "#shop: milk, eggs");
    let shop_b = capture(&paths, "#shop: milk, bread");
    capture_store::promote::settle(&paths).unwrap();

    let lines = ipc::list("shop".into()).expect("the list answers");
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert!(
        lines.iter().all(|l| !l.slug.is_empty()),
        "a line does not say which capture it came from: {lines:?}"
    );
    let from: std::collections::BTreeSet<&str> = lines.iter().map(|l| l.slug.as_str()).collect();
    assert_eq!(
        from,
        [shop_a.as_str(), shop_b.as_str()].into_iter().collect(),
        "the lines do not name both captures"
    );

    // Two items reading "milk", from different captures, told apart by where
    // they came from — which is the whole reason the slug is carried.
    let milk: Vec<&str> = lines
        .iter()
        .filter(|l| l.text == "milk")
        .map(|l| l.slug.as_str())
        .collect();
    assert_eq!(milk.len(), 2, "{lines:?}");
    assert_ne!(milk[0], milk[1], "identical items came from one capture");

    let _ = std::fs::remove_dir_all(&dir);
}

fn capture(paths: &capture_store::StorePaths, text: &str) -> String {
    let mut s = capture_store::session::Session::open_for_capture(paths.clone()).unwrap();
    let body = capture_store::session::capture_body(text).unwrap();
    s.capture(&body).unwrap().slug
}
