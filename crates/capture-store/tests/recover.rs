//! The copy kept before a redaction, and getting it back.
//!
//! §1.2 keeps a snapshot of what a redaction is about to destroy, for thirty
//! days, "so that a misfire is recoverable during the window in which you would
//! notice it" — explicitly for the case somebody notices long after the
//! cancellation window closed.
//!
//! Everything around it was built and the read had no caller. So a plaintext
//! copy of content somebody declared must not exist was kept for a month,
//! reported on the status page, and unusable: the cost paid in full and the
//! purpose unreachable.

use capture_store::session::Session;
use capture_store::{StorePaths, promote};

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "capture-recover-{tag}-{}-{:?}",
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

/// Ahead of the real clock, because the redaction is stamped from it. Not
/// `u64::MAX`: the drain adds to whatever it is given.
const PAST_THE_WINDOW: u64 = 2_000_000_000_000;

fn capture(paths: &StorePaths, text: &str) -> String {
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = capture_store::session::capture_body(text).unwrap();
    s.capture(&body).unwrap().slug
}

/// Redact and carry the destruction out.
fn destroy(paths: &StorePaths, slug: &str) {
    capture_store::mutate::redact(paths, slug).unwrap();
    let mut s = Session::open(paths.clone()).unwrap();
    assert!(s.drain_due_at(PAST_THE_WINDOW).unwrap().destroyed >= 1);
}

#[test]
fn a_destroyed_note_leaves_a_copy_that_can_be_listed() {
    let sc = Scratch::new("listed");
    let p = sc.paths();
    let slug = capture(&p, "the minutes of the board meeting");
    promote::settle(&p).unwrap();

    let ops = capture_store::ops::Ops::open_read(&p.ops).unwrap();
    assert!(
        ops.snapshots(PAST_THE_WINDOW).unwrap().is_empty(),
        "nothing is destroyed yet, so nothing is kept"
    );
    drop(ops);

    destroy(&p, &slug);

    let ops = capture_store::ops::Ops::open_read(&p.ops).unwrap();
    let kept = ops.snapshots(PAST_THE_WINDOW).unwrap();
    assert_eq!(kept.len(), 1, "the copy was taken and could not be listed");
    assert_eq!(kept[0].text, "the minutes of the board meeting");
}

#[test]
fn the_copy_outlives_the_cancellation_window() {
    // The whole argument for keeping one. §1.2: the window covers the common
    // case, and this exists for noticing a month later. The window is thirty
    // minutes — not the six hours DESIGN states in five places — which makes
    // the copy the larger half of the safety net rather than the smaller.
    let sc = Scratch::new("outlives");
    let p = sc.paths();
    let slug = capture(&p, "something destroyed by mistake");
    promote::settle(&p).unwrap();
    destroy(&p, &slug);

    let ops = capture_store::ops::Ops::open_read(&p.ops).unwrap();
    let kept = ops.snapshots(PAST_THE_WINDOW).unwrap();
    let window = capture_core::REDACT_WINDOW_MS;
    assert!(
        kept[0].expires_at - kept[0].taken_at > window,
        "the copy expires no later than the window it exists to outlast"
    );
}

#[test]
fn an_expired_copy_is_not_offered() {
    let sc = Scratch::new("expired");
    let p = sc.paths();
    let slug = capture(&p, "destroyed and long past keeping");
    promote::settle(&p).unwrap();
    destroy(&p, &slug);

    let ops = capture_store::ops::Ops::open_read(&p.ops).unwrap();
    let kept = ops.snapshots(PAST_THE_WINDOW).unwrap();
    let after = kept[0].expires_at + 1;
    assert!(
        ops.snapshots(after).unwrap().is_empty(),
        "a copy past its retention is still being offered"
    );
}

#[test]
fn recovering_writes_a_new_note_and_leaves_the_redaction_standing() {
    // Never a resurrection. The body is gone from the log and the redaction is
    // retained so it destroys matching events arriving later — including one
    // carrying this content from a peer. A command that said "restored" would
    // be claiming the redaction was undone, which is the more dangerous of the
    // two available lies: somebody would stop looking for the other copies.
    let sc = Scratch::new("new-note");
    let p = sc.paths();
    let slug = capture(&p, "the minutes of the board meeting");
    promote::settle(&p).unwrap();
    destroy(&p, &slug);

    let text = {
        let ops = capture_store::ops::Ops::open_read(&p.ops).unwrap();
        ops.snapshots(PAST_THE_WINDOW).unwrap()[0].text.clone()
    };

    let fresh = capture(&p, &text);
    assert_ne!(
        fresh, slug,
        "a recovered note is a new one, with its own name"
    );

    let store = capture_store::projection::current(&p).unwrap();
    let back = store
        .resolve(&fresh)
        .unwrap()
        .expect("the new note is named");
    assert_eq!(
        store.note(&back).unwrap().unwrap().text.as_deref(),
        Some("the minutes of the board meeting"),
    );

    // And the original is still destroyed.
    let gone = store.resolve(&slug).unwrap();
    if let Some(tgt) = gone {
        assert!(
            store.note(&tgt).unwrap().unwrap().redacted_at.is_some(),
            "the redaction stopped standing"
        );
    }
}
