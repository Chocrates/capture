//! A cancellation that arrived after the content was gone.
//!
//! §1.1: "a cancel arriving after its target has been destroyed is recorded and
//! reported rather than silently discarded". It was recorded — the event is in
//! the log like any other — and reported nowhere, because the fold computed
//! both halves and kept neither: the content is gone, the governing order was
//! withdrawn, and the destroyed branch simply set `redacted_at`.
//!
//! The situation is ordinary. Two devices, one redaction: you cancel on the
//! phone and the desktop has already destroyed. The phone says nothing was
//! destroyed, the desktop silently keeps it gone, and nothing reconciles them.

use capture_core::body::RedactCancelBody;
use capture_core::event::Kind;
use capture_store::StorePaths;
use capture_store::session::Session;

mod common;
use common::tgt_of;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "capture-late-{tag}-{}-{:?}",
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

const PAST_THE_WINDOW: u64 = 2_000_000_000_000;

fn capture(paths: &StorePaths, text: &str) -> String {
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = capture_store::session::capture_body(text).unwrap();
    s.capture(&body).unwrap().slug
}

/// The redaction event governing a note, as the queue names it.
fn queued_redaction(paths: &StorePaths) -> capture_core::EventId {
    capture_store::projection::current(paths)
        .unwrap()
        .redact_queue()
        .unwrap()
        .first()
        .map(|(redact, _, _, _)| *redact)
        .expect("a redaction is queued")
}

/// A cancellation arriving from somewhere else, written straight into the log.
///
/// `mutate::cancel_redaction` deliberately refuses once the destruction has
/// happened — that is the local half, fixed separately. This is the half that
/// arrives from a peer, which no local refusal can prevent.
fn cancel_from_a_peer(
    paths: &StorePaths,
    redact: capture_core::EventId,
    tgt: capture_core::EventId,
) {
    let mut s = Session::open(paths.clone()).unwrap();
    s.append(Kind::REDACT_CANCEL, Some(tgt), None, |nonce| {
        let mut b = Vec::new();
        minicbor::encode(&RedactCancelBody { nonce, redact }, &mut b).unwrap();
        b
    })
    .unwrap();
}

#[test]
fn a_cancellation_after_the_destruction_is_recorded() {
    let sc = Scratch::new("recorded");
    let p = sc.paths();
    let slug = capture(&p, "destroyed before the cancel arrived");
    capture_store::promote::settle(&p).unwrap();
    let tgt = tgt_of(&p, &slug);

    capture_store::mutate::redact(&p, &slug).unwrap();
    let redact = queued_redaction(&p);

    let mut s = Session::open(p.clone()).unwrap();
    assert_eq!(s.drain_due_at(PAST_THE_WINDOW).unwrap().destroyed, 1);
    drop(s);

    cancel_from_a_peer(&p, redact, tgt);

    let late = capture_store::projection::current(&p)
        .unwrap()
        .late_cancels()
        .unwrap();
    assert_eq!(late, vec![tgt], "the cancellation was discarded silently");
}

#[test]
fn a_cancellation_inside_the_window_is_not_late() {
    // The guard: cancelling in time must not look like cancelling too late,
    // and the note must survive.
    let sc = Scratch::new("in-time");
    let p = sc.paths();
    let slug = capture(&p, "saved by cancelling in time");
    capture_store::promote::settle(&p).unwrap();
    let tgt = tgt_of(&p, &slug);

    let id = capture_store::mutate::redact(&p, &slug).unwrap();
    capture_store::mutate::cancel_redaction(&p, &id).unwrap();

    let mut s = Session::open(p.clone()).unwrap();
    assert_eq!(s.drain_due_at(PAST_THE_WINDOW).unwrap().destroyed, 0);
    drop(s);

    let store = capture_store::projection::current(&p).unwrap();
    assert!(
        store.late_cancels().unwrap().is_empty(),
        "cancelling in time was reported as too late"
    );
    assert!(
        store.note(&tgt).unwrap().unwrap().text.is_some(),
        "the note was destroyed despite being cancelled in time"
    );
}

#[test]
fn a_destruction_nobody_cancelled_records_nothing() {
    let sc = Scratch::new("uncancelled");
    let p = sc.paths();
    let slug = capture(&p, "destroyed and never argued with");
    capture_store::promote::settle(&p).unwrap();

    capture_store::mutate::redact(&p, &slug).unwrap();
    let mut s = Session::open(p.clone()).unwrap();
    s.drain_due_at(PAST_THE_WINDOW).unwrap();
    drop(s);

    assert!(
        capture_store::projection::current(&p)
            .unwrap()
            .late_cancels()
            .unwrap()
            .is_empty(),
        "an ordinary destruction was reported as a late cancellation"
    );
}
