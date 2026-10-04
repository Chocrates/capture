//! A destruction happens once.
//!
//! `purge_note` edits the projection in place, so every derived table it should
//! clear has to be named there — which is why `mentions`, `list_items` and
//! `ambient` were each found surviving a purge in turn. `redact_queue` was the
//! fourth.
//!
//! The fold is right about this: once a body is gone it stops setting
//! `redaction_pending`, so a rebuilt projection has no row. Nothing deleted
//! what the fold would no longer write, so the row stayed and every later drain
//! destroyed the note again — including a write-ahead-log checkpoint, paid on
//! every drain for every redaction the archive had ever carried out.

use capture_store::StorePaths;
use capture_store::session::Session;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "capture-once-{tag}-{}-{:?}",
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

fn queued(paths: &StorePaths) -> usize {
    capture_store::projection::current(paths)
        .unwrap()
        .redact_queue()
        .unwrap()
        .len()
}

#[test]
fn a_carried_out_destruction_leaves_the_queue() {
    let sc = Scratch::new("leaves");
    let p = sc.paths();
    let slug = capture(&p, "a note to destroy");
    capture_store::promote::settle(&p).unwrap();
    capture_store::mutate::redact(&p, &slug).unwrap();
    assert_eq!(queued(&p), 1);

    let mut s = Session::open(p.clone()).unwrap();
    assert_eq!(s.drain_due_at(PAST_THE_WINDOW).unwrap().destroyed, 1);
    drop(s);

    assert_eq!(
        queued(&p),
        0,
        "the destruction stayed queued after happening"
    );
}

#[test]
fn a_second_drain_destroys_nothing() {
    // It used to destroy it again: record an intent, attempt a snapshot, delete
    // rows already gone, run an UPDATE that changed nothing, and checkpoint the
    // write-ahead log — every drain, for ever.
    let sc = Scratch::new("second");
    let p = sc.paths();
    let slug = capture(&p, "a note to destroy once");
    capture_store::promote::settle(&p).unwrap();
    capture_store::mutate::redact(&p, &slug).unwrap();

    let mut s = Session::open(p.clone()).unwrap();
    s.drain_due_at(PAST_THE_WINDOW).unwrap();
    drop(s);

    let mut s = Session::open(p.clone()).unwrap();
    assert_eq!(
        s.drain_due_at(PAST_THE_WINDOW).unwrap().destroyed,
        0,
        "a later drain destroyed something already destroyed"
    );
}

#[test]
fn other_pending_destructions_are_untouched() {
    // The guard against clearing the whole queue rather than one row.
    let sc = Scratch::new("others");
    let p = sc.paths();
    let a = capture(&p, "destroyed now");
    let b = capture(&p, "destroyed later");
    capture_store::promote::settle(&p).unwrap();
    capture_store::mutate::redact(&p, &a).unwrap();
    capture_store::mutate::redact(&p, &b).unwrap();
    assert_eq!(queued(&p), 2);

    // Only the first is due: the second's window has not elapsed at this clock.
    let queue = capture_store::projection::current(&p)
        .unwrap()
        .redact_queue()
        .unwrap();
    let earliest = queue.iter().map(|(_, _, after, _)| *after).min().unwrap();

    let mut s = Session::open(p.clone()).unwrap();
    s.drain_due_at(earliest).unwrap();
    drop(s);

    assert!(queued(&p) >= 1, "pending destructions were cleared too");
}

#[test]
fn cancelling_after_the_destruction_is_refused() {
    // It used to find the stale row, write the cancellation, and report
    // "Nothing was destroyed" over content that was gone. Being told a recovery
    // happened when it did not is worse than being told nothing, in the one
    // operation that cannot be walked back.
    let sc = Scratch::new("late-cancel");
    let p = sc.paths();
    let slug = capture(&p, "gone before the cancel");
    capture_store::promote::settle(&p).unwrap();
    let id = capture_store::mutate::redact(&p, &slug).unwrap();

    let mut s = Session::open(p.clone()).unwrap();
    s.drain_due_at(PAST_THE_WINDOW).unwrap();
    drop(s);

    let e = capture_store::mutate::cancel_redaction(&p, &id)
        .expect_err("cancelling a destruction that already happened must be refused");
    let said = format!("{e}");
    assert!(
        said.contains("already have been carried out"),
        "the refusal should say why: {said}"
    );
}

#[test]
fn cancelling_inside_the_window_still_works() {
    // The guard: the refusal above must not come from cancelling being broken.
    let sc = Scratch::new("in-window");
    let p = sc.paths();
    let slug = capture(&p, "saved in time");
    capture_store::promote::settle(&p).unwrap();
    let id = capture_store::mutate::redact(&p, &slug).unwrap();

    capture_store::mutate::cancel_redaction(&p, &id).expect("cancelling inside the window works");

    let mut s = Session::open(p.clone()).unwrap();
    assert_eq!(
        s.drain_due_at(PAST_THE_WINDOW).unwrap().destroyed,
        0,
        "a cancelled destruction was carried out anyway"
    );
}
