//! An hour without am or pm, from capture to a settled time.
//!
//! Found in use on 2026-10-12: "go to the gym every monday at 2" became a
//! series at 02:00. The recognizer now refuses to guess (capture-parse tests
//! that); these prove the rest of the path — the capture still lands on the
//! right day without a time, and answering the question is an ordinary edit
//! that re-promotes it with one.

#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "capturing stamps the real clock, as a surface would"
)]

use capture_store::StorePaths;
use capture_store::mutate::{Half, settle_hour};
use capture_store::session::Session;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("capture-hours-{name}-{}", std::process::id()));
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

/// The promotion's one-line rendering and the text, after settling.
fn read(paths: &StorePaths, slug: &str) -> (String, String) {
    let s = Session::open(paths.clone()).unwrap();
    let tgt = s.state.resolve(slug).unwrap().unwrap();
    let store = capture_store::projection::current(paths).unwrap();
    drop(s);
    let p = store.promotion_of(&tgt).unwrap().expect("promoted");
    let text = store.text_of(&tgt).unwrap().unwrap();
    (p.descriptor, text)
}

#[test]
fn the_gym_lands_on_mondays_with_no_time_until_asked() {
    let scratch = Scratch::new("gym");
    let paths = scratch.paths();
    let slug = capture(&paths, "go to the gym every monday at 2");

    let (descriptor, text) = read(&paths, &slug);
    assert!(descriptor.contains("Mon"), "{descriptor}");
    assert!(!descriptor.contains("02:00"), "guessed 2 am: {descriptor}");
    assert!(capture_parse::unclear_hour(&text).is_some());

    let settled = settle_hour(&paths, &slug, Half::Pm).unwrap();
    assert_eq!(settled, "go to the gym every monday at 2pm");

    let (descriptor, text) = read(&paths, &slug);
    assert!(descriptor.contains("14:00"), "{descriptor}");
    assert_eq!(
        capture_parse::unclear_hour(&text),
        None,
        "nothing left to ask"
    );
}

#[test]
fn the_dentist_keeps_its_day_and_takes_the_answer() {
    let scratch = Scratch::new("dentist");
    let paths = scratch.paths();
    let slug = capture(&paths, "dentist on oct 15th at 2");

    let (descriptor, _) = read(&paths, &slug);
    assert!(descriptor.contains("-10-15"), "{descriptor}");
    assert!(!descriptor.contains("02:00"), "guessed 2 am: {descriptor}");

    // Either answer is the person's; am is as valid as pm.
    settle_hour(&paths, &slug, Half::Am).unwrap();
    let (descriptor, text) = read(&paths, &slug);
    assert_eq!(text, "dentist on oct 15th at 2am");
    assert!(descriptor.contains("02:00"), "{descriptor}");
}

#[test]
fn nothing_to_ask_is_refused_rather_than_rewritten() {
    let scratch = Scratch::new("clear");
    let paths = scratch.paths();
    let slug = capture(&paths, "gym every monday at 2pm");
    assert!(settle_hour(&paths, &slug, Half::Pm).is_err());
    let (_, text) = read(&paths, &slug);
    assert_eq!(text, "gym every monday at 2pm", "the text was not touched");
}
