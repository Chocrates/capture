//! The ceiling on how much one origin may destroy.
//!
//! §1.1: "beyond 256 targets per hour from a single origin, further redactions
//! are held pending explicit confirmation rather than executed. The bound has
//! to be enforced by the receiver, since the buggy or compromised device is
//! exactly the one that will not enforce it on itself."
//!
//! It never fired once. `redaction_history` selected from `note_versions` for
//! `Kind::REDACT`, and that table only ever receives CAPTURE, EDIT and
//! TOMBSTONE — so the history was empty, `check` always said Proceed, and
//! `Drained.held` could never be non-zero. The limiter itself was correct and
//! well tested the whole time; what fed it was not.

use capture_store::ratelimit::TARGETS_PER_HOUR;
use capture_store::session::Session;
use capture_store::{StorePaths, promote};

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "capture-ceiling-{tag}-{}-{:?}",
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

/// Redact `n` notes from this device, all within the same moment.
///
/// One session for the whole batch rather than one per redaction, which is both
/// much faster and a truer model of what is being bounded: §1.1's threat is "a
/// loop emits a thousand redacts as easily as one", and a loop does not reopen
/// the store between them.
fn redact_many(paths: &StorePaths, n: usize) -> Vec<capture_core::EventId> {
    let mut slugs = Vec::new();
    for i in 0..n {
        slugs.push(capture(paths, &format!("note number {i}")));
    }
    promote::settle(paths).unwrap();

    let mut s = Session::open(paths.clone()).unwrap();
    let mut tgts = Vec::new();
    for slug in &slugs {
        let tgt = s.state.resolve(slug).unwrap().expect("the note is named");
        s.redact(tgt).unwrap();
        tgts.push(tgt);
    }
    tgts
}

/// Whether the bytes are still in the log.
///
/// The property that matters, and not the same as whether the note is readable:
/// a redaction hides its target from every view *on ingest*, immediately, and
/// only the destruction is delayed (§1.1). So a held note is already invisible
/// and its body is still there — which is the whole point of holding.
fn body_survives(paths: &StorePaths, tgt: &capture_core::EventId) -> bool {
    capture_store::log::Log::open_read(&paths.events)
        .unwrap()
        .by_id(tgt)
        .unwrap()
        .is_some_and(|e| e.body.is_some())
}

#[test]
fn an_origin_past_the_ceiling_is_held() {
    // The headline. Before this, 400 redactions in a minute produced a history
    // of zero rows and four hundred carried-out destructions.
    let sc = Scratch::new("held");
    let p = sc.paths();
    let over = TARGETS_PER_HOUR + 20;
    redact_many(&p, over);

    let mut s = Session::open(p.clone()).unwrap();
    let done = s.drain_due_at(PAST_THE_WINDOW).unwrap();
    drop(s);

    // Exactly the ceiling proceeds and the remainder waits. "At most" would
    // also be satisfied by holding everything, which is what the first
    // implementation did — each redaction counted all the others, so there was
    // no first 256 and a batch one over the line stopped completely.
    assert_eq!(
        done.destroyed, TARGETS_PER_HOUR,
        "the ones that fit under the ceiling did not proceed"
    );
    assert_eq!(done.held, over - TARGETS_PER_HOUR, "the rest were not held");
}

#[test]
fn what_is_held_still_has_its_content() {
    // Holding has to mean the content is still there. A ceiling that reported a
    // hold and destroyed anyway would be worse than none.
    let sc = Scratch::new("intact");
    let p = sc.paths();
    let tgts = redact_many(&p, TARGETS_PER_HOUR + 10);

    let mut s = Session::open(p.clone()).unwrap();
    let done = s.drain_due_at(PAST_THE_WINDOW).unwrap();
    drop(s);

    let surviving = tgts.iter().filter(|t| body_survives(&p, t)).count();
    assert_eq!(
        surviving, done.held,
        "the held destructions and the surviving bodies disagree"
    );
    assert!(done.held > 0, "nothing was held, so this proves nothing");
}

#[test]
fn ordinary_use_is_never_held() {
    // A ceiling ordinary use trips is one people learn to work around. Bulk
    // clearing is tens of notes, which §1.1 puts far below the bound.
    let sc = Scratch::new("ordinary");
    let p = sc.paths();
    redact_many(&p, 40);

    let mut s = Session::open(p.clone()).unwrap();
    let done = s.drain_due_at(PAST_THE_WINDOW).unwrap();
    assert_eq!(done.held, 0, "ordinary bulk use was held");
    assert_eq!(done.destroyed, 40);
}

#[test]
fn a_clustered_batch_drained_much_later_is_still_held() {
    // The case the old shape got wrong in principle as well as in fact. The
    // window used to be measured from the drain, so a runaway's output arriving
    // on a replica after a month offline scored nothing and all proceeded —
    // which is the receiving-replica case the bound exists for.
    //
    // Measured around the redaction's own timestamp, the cluster is a fact
    // about when it was issued and stays true however late the drain happens.
    let sc = Scratch::new("late");
    let p = sc.paths();
    redact_many(&p, TARGETS_PER_HOUR + 20);

    // A year after the redactions were written.
    let much_later = PAST_THE_WINDOW + 365 * 24 * 60 * 60 * 1000;
    let mut s = Session::open(p.clone()).unwrap();
    let done = s.drain_due_at(much_later).unwrap();
    assert!(
        done.held > 0,
        "a clustered batch drained a year later was not held"
    );
}

#[test]
fn releasing_lets_it_through() {
    // §1.1's "explicit confirmation". Without it the ceiling is a wall rather
    // than a pause, and a person who meant it has no way forward.
    let sc = Scratch::new("release");
    let p = sc.paths();
    let tgts = redact_many(&p, TARGETS_PER_HOUR + 5);

    let mut s = Session::open(p.clone()).unwrap();
    let first = s.drain_due_at(PAST_THE_WINDOW).unwrap();
    drop(s);
    assert!(first.held > 0);

    let ops = capture_store::ops::Ops::open(&p.ops).unwrap();
    let held = ops.held_redactions().unwrap();
    assert_eq!(held.len(), first.held, "what was held was not recorded");
    for (redact, _) in &held {
        ops.release_redaction(redact, PAST_THE_WINDOW).unwrap();
    }
    drop(ops);

    let mut s = Session::open(p.clone()).unwrap();
    let second = s.drain_due_at(PAST_THE_WINDOW).unwrap();
    assert_eq!(second.held, 0, "a released redaction was held again");

    // By the bytes rather than by the count. A destroyed note keeps its row in
    // `redact_queue`, so a later drain re-runs the purge idempotently and
    // counts it again — which makes `destroyed` a poor thing to assert on and
    // is worth knowing about separately.
    assert!(
        tgts.iter().all(|t| !body_survives(&p, t)),
        "something the release covered still has its body"
    );
}
