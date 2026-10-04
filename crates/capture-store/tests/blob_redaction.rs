//! Redaction destroys the recording, not only the words.
//!
//! DESIGN §1.1 states the behaviour as implemented: *"blob redaction overwrites
//! the file before unlinking"*. `blobs::destroy` does exactly that, is tested,
//! and `purge.rs` never called it — so redacting a voice note destroyed the
//! event body and left the audio on disk, in the one operation whose entire
//! purpose is destruction.
//!
//! `tools/unwired.py` should have caught it and could not: a string literal in
//! a confirmation prompt, "This will permanently destroy the content of:",
//! contains the word, and the scan strips comments but not strings.

use capture_core::body::BlobRef;
use capture_store::session::Session;
use capture_store::{StorePaths, blobs, promote};

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "capture-blobredact-{tag}-{}-{:?}",
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

/// A voice note: bytes in the blob store, and a capture referencing them.
fn voice(paths: &StorePaths, bytes: &[u8]) -> (String, [u8; 32]) {
    let hash = blobs::put(&paths.blobs, bytes).unwrap();
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = capture_store::session::capture_body_with_blob(
        "",
        BlobRef {
            hash,
            media_type: "audio/ogg".into(),
            bytes: bytes.len() as u64,
            duration_ms: None,
        },
    )
    .unwrap();
    (s.capture(&body).unwrap().slug, hash)
}

/// Redact, and carry out the deferred destruction immediately.
fn redact_now(paths: &StorePaths, slug: &str) {
    capture_store::mutate::redact(paths, slug).unwrap();
    // The window is cancellable (§1.1), so nothing is destroyed until it
    // elapses. Draining past it is what the overlay tick and every session open
    // do once the time comes.
    //
    // A fixed instant well ahead of the real clock, not `u64::MAX`: the drain
    // adds to whatever it is given, and the maximum overflows. The redaction is
    // stamped from the real clock even though the fixture's events are not, so
    // this has to be ahead of *now* rather than ahead of the fixture.
    const WELL_PAST_THE_WINDOW: u64 = 2_000_000_000_000;
    let mut s = Session::open(paths.clone()).unwrap();
    let done = s.drain_due_at(WELL_PAST_THE_WINDOW).unwrap();
    // At least one, not exactly one: a drain carries out everything that is
    // due, and a test that redacts twice may find both on the same pass. What
    // each case actually asserts is about the bytes.
    assert!(done.destroyed >= 1, "the redaction did not run");
}

#[test]
fn redacting_a_voice_note_destroys_the_recording() {
    let sc = Scratch::new("destroys");
    let p = sc.paths();
    let (slug, hash) = voice(&p, b"pretend this is audio");
    assert!(
        blobs::have(&p.blobs, &hash),
        "the bytes should be there first"
    );

    redact_now(&p, &slug);

    assert!(
        !blobs::have(&p.blobs, &hash),
        "the event body was destroyed and the recording was left on disk"
    );
}

#[test]
fn a_recording_another_note_still_references_is_kept() {
    // The hash is the address, so one recording captured twice is one blob with
    // two references (§5.6). Destroying it because one of them was redacted
    // would destroy content nobody asked to lose — the cardinal failure,
    // performed by the operation least able to undo it.
    let sc = Scratch::new("shared");
    let p = sc.paths();
    let (first, hash) = voice(&p, b"one recording, two notes");
    let (second, same) = voice(&p, b"one recording, two notes");
    assert_eq!(hash, same, "identical bytes are one blob");

    redact_now(&p, &first);
    assert!(
        blobs::have(&p.blobs, &hash),
        "the other note still references it"
    );

    redact_now(&p, &second);
    assert!(
        !blobs::have(&p.blobs, &hash),
        "the last reference went and the bytes should have gone with it"
    );
}

#[test]
fn a_note_with_no_recording_redacts_as_before() {
    let sc = Scratch::new("plain");
    let p = sc.paths();
    let slug = {
        let mut s = Session::open_for_capture(p.clone()).unwrap();
        let body = capture_store::session::capture_body("an ordinary note").unwrap();
        s.capture(&body).unwrap().slug
    };
    promote::settle(&p).unwrap();

    redact_now(&p, &slug);

    // `redact_now` already asserts the drain destroyed something. What this
    // case is for is that the blob step does not disturb the ordinary path:
    // the body is gone from the log, and nothing went looking for bytes that
    // were never there.
    let log = capture_store::log::Log::open_read(&p.events).unwrap();
    let captures_with_text = log
        .all()
        .unwrap()
        .iter()
        .filter(|e| e.header.kind == capture_core::event::Kind::CAPTURE)
        .filter(|e| e.body.is_some())
        .count();
    assert_eq!(captures_with_text, 0, "the capture body survived redaction");
}

#[test]
fn destroying_a_recording_twice_is_not_an_error() {
    // What makes the resume path work: a purge interrupted after the blobs and
    // before the bodies re-runs from the top, and finds the bytes already gone.
    let sc = Scratch::new("twice");
    let p = sc.paths();
    let (slug, hash) = voice(&p, b"audio");

    redact_now(&p, &slug);
    assert!(!blobs::have(&p.blobs, &hash));

    // Re-running the whole purge is exactly what `resume` does.
    drop(Session::open(p.clone()).unwrap());
    assert!(!blobs::have(&p.blobs, &hash));
}
