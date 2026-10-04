//! Something has to read a capture, and a read has to be able to say nothing has.
//!
//! The bug these cover, reproduced on a scratch store before any of this
//! existed: capture a dated task, ask for your tasks, and be told there are
//! none — twice — until some unrelated write happens to open the store and
//! sweep on the way in.
//!
//! §3.1 is why the parse is deferred and that is not in question. What was
//! missing is an owner for the deferred work, and a way for a read to admit it
//! is looking at state nobody has caught up yet. §8.8 constrains the second
//! half hard: a read must not involve the writer, so the admission has to be
//! answerable from a read-only handle *while the writer lock is held*. That is
//! the property most of this file is about.

// Reading a clock, which the purity lint otherwise refuses. Timing a sweep is
// not stamping an event, and the boundary that rule protects is the fold's.
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "measuring how long a sweep takes, not stamping an event"
)]

use capture_store::session::Session;
use capture_store::{StorePaths, promote};

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "capture-settle-{tag}-{}-{:?}",
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

/// A capture, by the path a capture actually takes.
///
/// `open_for_capture` deliberately does not sweep — that is §3.1's budget — so
/// this is the only way to set up the state the bug lives in. Using
/// `Session::open` here would sweep on the way in and quietly make every test
/// below pass for the wrong reason.
fn capture(paths: &StorePaths, text: &str) -> String {
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = capture_store::session::capture_body(text).unwrap();
    s.capture(&body).unwrap().slug
}

fn unread(paths: &StorePaths) -> usize {
    capture_store::projection::current(paths)
        .unwrap()
        .unread_count(capture_parse::GRAMMAR_VERSION)
        .unwrap()
}

// ------------------------------------------------------------ settling ------

#[test]
fn a_capture_is_not_read_until_something_reads_it() {
    let s = Scratch::new("deferred");
    let p = s.paths();
    capture(&p, "call the dentist tomorrow");

    assert_eq!(
        unread(&p),
        1,
        "a fresh capture has not been read by anything"
    );

    let n = promote::settle(&p).unwrap();
    assert_eq!(n, 1, "settling reads it and says that it did");
    assert_eq!(unread(&p), 0, "and nothing is outstanding afterwards");
}

#[test]
fn settling_twice_reads_nothing_the_second_time() {
    let s = Scratch::new("idempotent");
    let p = s.paths();
    capture(&p, "call the dentist tomorrow");

    assert_eq!(promote::settle(&p).unwrap(), 1);
    assert_eq!(
        promote::settle(&p).unwrap(),
        0,
        "nothing left to read, and it says zero rather than re-reading"
    );
}

#[test]
fn settling_reports_what_it_read_not_what_it_opened() {
    // The count comes from the sweep the session runs on the way in. An earlier
    // draft swept *again* after opening, which read the whole backlog twice and
    // then reported zero — the first pass having already dealt with all of it.
    // Wrong in the direction that hides the bug, so it is pinned here.
    let s = Scratch::new("counts");
    let p = s.paths();
    for d in 1..=3 {
        capture(&p, &format!("call the dentist on 2026-04-{d:02}"));
    }
    assert_eq!(unread(&p), 3);
    assert_eq!(
        promote::settle(&p).unwrap(),
        3,
        "three were read, and three is what it should say"
    );
}

#[test]
fn the_count_settle_reports_is_the_count_a_read_warned_about() {
    // Otherwise the two halves of this change contradict each other on screen:
    // "2 captures have not been read" followed by "read 1 capture", with
    // nothing to explain the difference. Most captures are ordinary notes that
    // promote to nothing, so counting promotions here would make that the
    // normal case rather than an edge.
    let s = Scratch::new("agree");
    let p = s.paths();
    capture(&p, "buy milk");
    capture(&p, "dentist on 2026-10-20");

    let warned = unread(&p);
    assert_eq!(warned, 2, "two captures, neither read");
    assert_eq!(
        promote::settle(&p).unwrap(),
        warned,
        "and settling reports the same number the warning named, \
         though only one of the two promoted to anything"
    );
}

#[test]
fn a_held_writer_lock_is_an_error_and_not_a_zero() {
    // The distinction `cli::due` could not make: it opened a throwaway session
    // and dropped the `Result`, so a store somebody else was writing produced
    // "nothing to do" and an answer from stale state.
    let s = Scratch::new("contended");
    let p = s.paths();
    capture(&p, "call the dentist tomorrow");

    let held = Session::open_for_capture(p.clone()).unwrap();
    let r = promote::settle(&p);
    assert!(
        r.is_err(),
        "a store being written by somebody else is a failure to report, not a zero: {r:?}"
    );
    drop(held);

    assert_eq!(
        promote::settle(&p).unwrap(),
        1,
        "and once the lock is free the work is still there to do"
    );
}

// ------------------------------------------------- saying so, read-only -----

#[test]
fn staleness_is_answerable_with_the_writer_lock_held() {
    // §8.8, directly. A surface that can only report staleness while the writer
    // is healthy cannot report it at the moment it matters — which is precisely
    // when something holds the store and has not finished with it.
    let s = Scratch::new("locked");
    let p = s.paths();
    capture(&p, "call the dentist tomorrow");

    let held = Session::open_for_capture(p.clone()).unwrap();
    assert_eq!(
        unread(&p),
        1,
        "the count is a read-only query and the lock is none of its business"
    );
    drop(held);
}

#[test]
fn nothing_outstanding_counts_zero() {
    let s = Scratch::new("clean");
    let p = s.paths();
    capture(&p, "a plain note with nothing temporal in it");
    promote::settle(&p).unwrap();
    assert_eq!(
        unread(&p),
        0,
        "a note the recognizer declines to promote is still a note it has read"
    );
}

#[test]
fn a_note_that_promotes_to_nothing_is_still_read_once() {
    // Otherwise the line never clears. An ordinary capture clears no floor, so
    // if "read" meant "promoted" every archive would permanently report a
    // backlog it could never work off — a warning that no amount of sweeping
    // silences, which is worse than no warning.
    let s = Scratch::new("ordinary");
    let p = s.paths();
    capture(&p, "thinking about the garden again");

    assert_eq!(unread(&p), 1, "it has not been looked at yet");
    promote::settle(&p).unwrap();
    assert_eq!(
        unread(&p),
        0,
        "looked at, found nothing, and not asked about again"
    );
}

#[test]
fn an_edit_makes_a_capture_unread_again() {
    // The fields going stale against their own text is what the sweep's
    // `updated` comparison exists for, and the count has to agree with it or
    // the surface says "nothing outstanding" over a note whose promotion
    // contradicts what it now says.
    let s = Scratch::new("edited");
    let p = s.paths();
    let slug = capture(&p, "dentist on 2026-04-01");
    promote::settle(&p).unwrap();
    assert_eq!(unread(&p), 0);

    capture_store::mutate::edit(&p, &slug, "dentist on 2026-04-08").unwrap();
    assert_eq!(
        unread(&p),
        1,
        "the text moved, so what read it is out of date"
    );
}

#[test]
fn an_ordinary_note_leaves_the_queue_once_it_has_been_read() {
    // This was the bug, and it was found by the test above asking whether the
    // staleness line ever clears. A note the recognizer declines writes no
    // promotion — correctly, since an event per ordinary capture is not worth
    // having — and the queue read a missing promotion as "not yet read". So
    // every ordinary note stayed in it for ever.
    let s = Scratch::new("permanent");
    let p = s.paths();
    capture(&p, "thinking about the garden again");
    promote::settle(&p).unwrap();

    let store = capture_store::projection::current(&p).unwrap();
    assert!(
        store
            .needing_parse(capture_parse::GRAMMAR_VERSION, 500)
            .unwrap()
            .is_empty(),
        "read and found to be nothing is still read, so the queue is empty"
    );
}

#[test]
fn a_capture_behind_two_hundred_ordinary_notes_is_still_read() {
    // The cost of an infinite queue, which is not a slow sweep but a *silent
    // horizon*: `ORDER BY created_wall DESC LIMIT 200` over a queue that never
    // empties means the newest two hundred unpromotable notes are re-read on
    // every open and nothing behind them is ever reached.
    //
    // Measured before the fix — one dated task, then 250 ordinary notes, then
    // five sweeps: nothing promoted, 251 still queued, and the task that was
    // captured *first* was never read at all. Any archive past a couple of
    // hundred notes had a sweep that did no useful work and a grammar
    // improvement that reached nothing.
    let s = Scratch::new("starved");
    let p = s.paths();
    capture(&p, "call the dentist on 2026-04-01");
    for i in 0..250 {
        capture(&p, &format!("thinking about the garden, note {i}"));
    }

    // Two sweeps, because 251 notes against `PER_SWEEP` = 200 is deliberately
    // more than one pass. The bound spreading work over runs is the designed
    // behaviour; the queue never draining was not.
    promote::settle(&p).unwrap();
    promote::settle(&p).unwrap();

    let store = capture_store::projection::current(&p).unwrap();
    assert_eq!(
        store.promoted("task", 500).unwrap().len(),
        1,
        "the oldest capture is reached, not starved behind the newest 200"
    );
    assert_eq!(
        unread(&p),
        0,
        "and the queue drains rather than standing at 251 for ever"
    );
}

/// What a sweep costs inside the writer lock, at the bound it runs at.
///
/// Worth measuring because this change made the sweep *do* something. Before
/// it, a full queue of unpromotable notes was re-read and nothing was written,
/// so the cost was a parse per note and no I/O; now a first pass over an
/// existing archive writes promotions and a `read_by` row per note, and it
/// does it on the daemon's round with the writer lock held. A capture during
/// that window spools rather than failing, which is the behaviour the blob
/// window already depends on — but spooling for a long time is still something
/// somebody would feel.
///
/// Ignored, and deliberately not a threshold. A number that fails CI on a busy
/// machine teaches people to re-run CI.
///
/// Measured, release, 250 captures of which 50 promote:
///
/// ```text
///   first pass  (40 promoted, 200 read): 2.86s
///   second pass (10 promoted,  50 read): 753ms
///   nothing to do:                        27ms
/// ```
///
/// So ~14ms per note read, and the steady state — which is what the daemon
/// does on almost every round — is 27ms. `PER_SWEEP` stays at 200: the pass is
/// long, but it is a catch-up that happens once per archive and a capture
/// during it spools rather than failing. Lowering the bound would trade a
/// shorter lock for a backlog that takes four times as many rounds to clear,
/// and the number to beat is recorded here for whoever decides otherwise.
#[test]
#[ignore = "a measurement, not an assertion"]
fn what_a_full_sweep_costs() {
    let s = Scratch::new("cost");
    let p = s.paths();

    // Past `PER_SWEEP` = 200, so this is a full pass at the bound, and a mix
    // of promotable and not because that is what an archive is.
    for i in 0..250 {
        if i % 5 == 0 {
            capture(
                &p,
                &format!("call the dentist on 2026-04-{:02}", (i % 28) + 1),
            );
        } else {
            capture(&p, &format!("thinking about the garden, note {i}"));
        }
    }

    let t = std::time::Instant::now();
    let first = promote::settle(&p).unwrap();
    let full = t.elapsed();

    let t = std::time::Instant::now();
    let second = promote::settle(&p).unwrap();
    let rest = t.elapsed();

    let t = std::time::Instant::now();
    promote::settle(&p).unwrap();
    let empty = t.elapsed();

    println!("  first pass  ({first:>3} promoted, 200 read): {full:?}");
    println!("  second pass ({second:>3} promoted,  50 read): {rest:?}");
    println!("  nothing to do:                        {empty:?}");
    println!("  unread after: {}", unread(&p));
}

#[test]
fn a_note_already_promoted_by_this_grammar_is_not_read_again() {
    // `read_by` is local and is dropped on a projection rebuild, while
    // `promotions` is rebuilt from the log. So after a rebuild — and on a
    // second device, where a promotion arrives by sync and no local read was
    // ever recorded — every promoted note has a current promotion and no
    // `read_by` row.
    //
    // If a missing `read_by` row alone meant "unread", all of those would be
    // swept again and each would write a *duplicate* promote event saying
    // exactly what the existing one says. Every rebuild would grow the log by
    // one event per promoted note and sync the lot to every peer.
    //
    // So a current promotion counts as having been read, the same as a
    // `read_by` row does.
    let s = Scratch::new("rebuilt");
    let p = s.paths();
    capture(&p, "call the dentist on 2026-04-01");
    promote::settle(&p).unwrap();

    let before = {
        let log = capture_store::log::Log::open_read(&p.events).unwrap();
        log.count().unwrap()
    };

    // An actual rebuild, not a simulated one. `rebuild_projection` re-folds
    // the log into a fresh generation, so `promotions` comes back and
    // `read_by` — which the fold does not produce — does not. That is the
    // state this is about, and doing it for real means no test-only door into
    // the projection.
    {
        let mut s = Session::open(p.clone()).unwrap();
        s.rebuild_projection().unwrap();
    }

    assert_eq!(
        unread(&p),
        0,
        "a promotion from this grammar is a record that the note was read"
    );
    assert_eq!(promote::settle(&p).unwrap(), 0, "so nothing is re-promoted");

    let after = {
        let log = capture_store::log::Log::open_read(&p.events).unwrap();
        log.count().unwrap()
    };
    assert_eq!(
        before, after,
        "and the log did not grow by a promotion saying what the last one said"
    );
}
