//! Review, as a session rather than a queue.
//!
//! The unit tests pin the numbers. These check the behaviours §2.7 and §2.8
//! actually promise: that the session ends, that muting wins, that two
//! deferrals stop an item being pressed, and that nothing anywhere accumulates
//! into a judgement about the person using it.

// Reading the clock, because the events under test are stamped with it. The
// crate-wide ban exists so the fold cannot consult one; this is a test
// comparing against times that were already written.
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "comparing against timestamps the store already wrote"
)]

use capture_core::body::Outcome;
use capture_store::session::Session;
use capture_store::{StorePaths, review};

const DAY: i64 = 24 * 60 * 60 * 1000;

/// The clock the records are stamped with.
///
/// Not a fixed date. A `review` event carries the real wall clock, so a test
/// that recorded an outcome and then asked "is it due a day later" against a
/// fixed June instant was asking about two months *before* the event it had
/// just written — and every interval looked un-elapsed. Selection compares
/// against what was actually written, so the test has to as well.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("capture-review-{name}-{}", std::process::id()));
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

fn offered(paths: &StorePaths, now: i64) -> Vec<String> {
    let s = Session::open_for_capture(paths.clone()).unwrap();
    review::session(&s, now)
        .unwrap()
        .into_iter()
        .map(|i| i.note.slug)
        .collect()
}

#[test]
fn a_session_is_bounded_and_ends() {
    // The property that keeps it from becoming a backlog. §3.6 spends its
    // length on why a manufactured backlog is the wrong shape for this user.
    let sc = Scratch::new("bounded");
    let p = sc.paths();
    for i in 0..20 {
        capture(&p, &format!("note number {i}"));
    }
    assert_eq!(offered(&p, now()).len(), review::SESSION);
}

#[test]
fn an_offered_note_is_not_offered_again_immediately() {
    // Otherwise the same five notes are offered forever and nothing else is
    // ever reached.
    let sc = Scratch::new("interval");
    let p = sc.paths();
    let slug = capture(&p, "the only note");

    assert_eq!(offered(&p, now()), std::slice::from_ref(&slug));
    review::record(&p, &slug, Outcome::Shown).unwrap();
    assert!(
        offered(&p, now()).is_empty(),
        "it was offered twice at once"
    );

    // A day later it is due again.
    assert_eq!(offered(&p, now() + DAY + 1000), [slug]);
}

#[test]
fn engaging_with_something_makes_it_recur_less_often() {
    // §2.8: an item you engage with recurs less often because it has landed.
    let sc = Scratch::new("engaged");
    let p = sc.paths();
    let slug = capture(&p, "a note worth keeping");

    review::record(&p, &slug, Outcome::Engaged).unwrap();
    assert!(
        offered(&p, now() + DAY + 1000).is_empty(),
        "one day was enough after engaging, so the interval did not expand"
    );
    assert_eq!(offered(&p, now() + 4 * DAY), [slug]);
}

#[test]
fn deferring_twice_stops_it_being_offered() {
    // Not pressed a third time. Somebody who has declined an item twice has
    // told you, and §2.7 exists to stop the system arguing.
    let sc = Scratch::new("deferred");
    let p = sc.paths();
    let slug = capture(&p, "not now, thanks");

    review::record(&p, &slug, Outcome::Deferred).unwrap();
    assert_eq!(
        offered(&p, now() + 2 * DAY),
        std::slice::from_ref(&slug),
        "once is not a refusal"
    );

    review::record(&p, &slug, Outcome::Deferred).unwrap();
    assert!(
        offered(&p, now() + 400 * DAY).is_empty(),
        "it was offered after being declined twice"
    );
}

#[test]
fn muting_always_wins() {
    // §2.6. Anything you do not want resurfaced stops being resurfaced,
    // reversibly, without being destroyed.
    let sc = Scratch::new("muted");
    let p = sc.paths();
    let slug = capture(&p, "stop showing me this");

    assert_eq!(offered(&p, now()), std::slice::from_ref(&slug));
    capture_store::mutate::mute_note(&p, &slug).unwrap();
    assert!(offered(&p, now()).is_empty(), "a muted note was resurfaced");

    // Still there, and still readable. Muting is not destruction.
    let s = Session::open_for_capture(p.clone()).unwrap();
    let tgt = s.state.resolve(&slug).unwrap().unwrap();
    assert!(s.state.text_of(&tgt).unwrap().is_some());
}

#[test]
fn a_forward_marker_is_offered_before_anything_else() {
    // §2.8's first tier, and the strongest signal there is: somebody said, in
    // the note itself, that this was worth coming back to.
    let sc = Scratch::new("forward");
    let p = sc.paths();
    for i in 0..4 {
        capture(&p, &format!("an ordinary note {i}"));
    }
    // An offset, not a weekday. A weekday resolves to a day this week, so
    // "dentist thursday 2pm" is only in the future for part of Thursday — this
    // test passed all morning and failed after two in the afternoon, and only
    // stopped hiding that when the sweep began reading notes against their own
    // capture time instead of an injected June clock.
    let forward = capture(&p, "dentist in 3 weeks");

    // Promote it, which is what makes the forward date visible to selection.
    let mut s = Session::open_for_capture(p.clone()).unwrap();
    capture_store::promote::sweep(&mut s).unwrap();
    drop(s);

    let session = offered(&p, now());
    assert_eq!(
        session.first().map(String::as_str),
        Some(forward.as_str()),
        "a forward marker did not come first: {session:?}"
    );
}

#[test]
fn nothing_accumulates_into_a_judgement() {
    // §2.7, checked at the surface rather than in the source. Whatever a
    // session records, there is no place to ask how well somebody is doing.
    let sc = Scratch::new("no-score");
    let p = sc.paths();
    let slug = capture(&p, "a note");

    for _ in 0..3 {
        review::record(&p, &slug, Outcome::Engaged).unwrap();
    }

    // The store exposes what was done, and offers no reading of it.
    let s = Session::open_for_capture(p.clone()).unwrap();
    let all = s.log.all().unwrap();
    let folded = capture_core::fold::fold(&all);
    let tgt = s.state.resolve(&slug).unwrap().unwrap();
    let n = &folded.notes[&tgt];

    assert_eq!(
        n.review.engaged, 3,
        "engagement is counted, which scheduling needs"
    );
    assert_eq!(n.review.deferred, 0);
    // And that is the whole of it: there is no third number, and nothing
    // derives one. The type has three fields and none of them is a verdict.
}

#[test]
fn a_batch_of_offers_keeps_the_sequence_gap_free() {
    // Writing the five `shown` events one at a time cost five commits — about
    // four hundred milliseconds before the pane could draw, which lands as a
    // delay on opening a tab.
    //
    // The first attempt at batching authored all five and then appended them,
    // which is wrong: an event's sequence and chain are read back from the log,
    // so five authored before any was appended all claimed the same number. The
    // store's own integrity check caught it — "local append out of sequence" —
    // and this is that check as a test rather than as a lucky console message.
    let sc = Scratch::new("batch");
    let p = sc.paths();
    let slugs: Vec<String> = (0..5).map(|i| capture(&p, &format!("note {i}"))).collect();

    let mut s = Session::open(p.clone()).unwrap();
    let tgts: Vec<_> = slugs
        .iter()
        .map(|x| s.state.resolve(x).unwrap().unwrap())
        .collect();
    review::shown(&mut s, &tgts).unwrap();
    drop(s);

    // Every sequence used once, with no gaps, for every origin.
    let s = Session::open(p.clone()).unwrap();
    let mut by_dev: std::collections::BTreeMap<String, Vec<u64>> = Default::default();
    for e in s.log.all().unwrap() {
        let dev = e.header.dev.0.iter().map(|b| format!("{b:02x}")).collect();
        by_dev.entry(dev).or_default().push(e.header.seq);
    }
    for (dev, mut seqs) in by_dev {
        seqs.sort_unstable();
        let wanted: Vec<u64> = (1..=seqs.len() as u64).collect();
        assert_eq!(seqs, wanted, "{dev} has a gap or a repeat in its sequence");
    }
    // The writer lock is held for as long as the session is, and `offered`
    // opens its own.
    drop(s);

    // And all five were recorded, not just the last one to win a collision.
    let offered = offered(&p, now());
    assert!(
        offered.is_empty(),
        "notes just offered are still on offer, so the writes did not land: {offered:?}"
    );
}
