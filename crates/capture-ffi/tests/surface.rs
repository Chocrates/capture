//! Every operation the phone can perform, called directly.
//!
//! Before any Kotlin exists, and deliberately so. A binding exercised only
//! through a screen is one whose failures arrive as a blank screen: the
//! interesting question — did the store do the right thing — is then two
//! layers and a language away from where it is answered.

use capture_ffi::{Archive, Captured};

fn scratch(name: &str) -> String {
    let d = std::env::temp_dir().join(format!(
        "capture-ffi-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    d.to_string_lossy().to_string()
}

#[test]
fn opening_an_empty_directory_creates_an_archive() {
    // The phone's first run. There is no `capture init` on Android — the
    // application is opened and there has to be somewhere to put things.
    let root = scratch("fresh");
    let a = Archive::open(root.clone()).expect("opened");

    assert_eq!(a.note_count().unwrap(), 0);
    assert_eq!(a.device_key().unwrap().len(), 12, "six bytes, as hex");

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn opening_twice_keeps_the_same_device() {
    // A phone is a device, not a session. Minting a new identity on every open
    // would give one machine many origins, and every peer would have to be
    // told about each of them.
    let root = scratch("same-device");
    let first = Archive::open(root.clone()).unwrap().device_key().unwrap();
    let second = Archive::open(root.clone()).unwrap().device_key().unwrap();

    assert_eq!(first, second);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_capture_is_readable_immediately() {
    let root = scratch("capture");
    let a = Archive::open(root.clone()).unwrap();

    let slug = match a.capture("milk and eggs".into()).unwrap() {
        Captured::Stored { slug } => slug,
        Captured::Spooled => panic!("spooled with a writable store"),
    };

    let note = a.note(slug.clone()).unwrap();
    assert_eq!(note.text.as_deref(), Some("milk and eggs"));
    assert_eq!(note.slug, slug);
    assert_eq!(a.note_count().unwrap(), 1);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_capture_survives_the_handle_being_dropped() {
    // The durability promise, in the shape Android actually produces: a Quick
    // Settings tile is destroyed the moment its callback returns. If anything
    // were queued on the caller's side rather than committed, this is where it
    // would be lost.
    let root = scratch("durable");
    {
        let a = Archive::open(root.clone()).unwrap();
        a.capture("written by something about to die".into())
            .unwrap();
    }

    let reopened = Archive::open(root.clone()).unwrap();
    assert_eq!(reopened.note_count().unwrap(), 1);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn the_timeline_is_newest_first() {
    let root = scratch("timeline");
    let a = Archive::open(root.clone()).unwrap();
    a.capture("first".into()).unwrap();
    a.capture("second".into()).unwrap();
    a.capture("third".into()).unwrap();

    let rows = a.timeline(10).unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].text.as_deref(), Some("third"));

    assert_eq!(a.timeline(2).unwrap().len(), 2, "the limit is honoured");
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn search_finds_what_was_captured() {
    let root = scratch("search");
    let a = Archive::open(root.clone()).unwrap();
    a.capture("buy sourdough starter".into()).unwrap();
    a.capture("something else entirely".into()).unwrap();

    let hits = a.search("sourdough".into(), 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].text.as_deref().unwrap().contains("sourdough"));

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn asking_for_a_note_that_is_not_there_says_so() {
    // Distinguishable from a store failure. A caller that cannot tell those
    // apart will report both as a crash, and one of them is ordinary.
    let root = scratch("missing");
    let a = Archive::open(root.clone()).unwrap();

    let e = a.note("no-such-note".into()).unwrap_err();
    assert!(
        matches!(e, capture_ffi::CaptureError::NotFound { .. }),
        "expected NotFound, got {e:?}"
    );

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn concurrent_captures_do_not_report_a_false_conflict() {
    // Several coroutines calling in at once is the ordinary case on Android.
    // The store's writer lock protects *between* processes; two callers inside
    // one process racing for it would otherwise produce "somebody else is
    // writing" for the only process on the device.
    use std::sync::Arc;

    let root = scratch("concurrent");
    let a = Arc::new(Archive::open(root.clone()).unwrap());

    let mut threads = Vec::new();
    for i in 0..8 {
        let a = Arc::clone(&a);
        threads.push(std::thread::spawn(move || {
            a.capture(format!("note number {i}")).unwrap()
        }));
    }
    for t in threads {
        match t.join().expect("no panic") {
            Captured::Stored { .. } => {}
            Captured::Spooled => panic!("spooled — a writable store reported a conflict"),
        }
    }

    assert_eq!(a.note_count().unwrap(), 8);
    std::fs::remove_dir_all(&root).ok();
}
