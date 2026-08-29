//! Promotion end to end: capture, sweep, correct, reparse.
//!
//! The unit tests decide what a recognition *would* become. These check what
//! actually lands in the log, because the three properties that matter are all
//! about the log rather than the parse: the captured text is never touched, a
//! correction is never undone by a later sweep, and nothing here happens on the
//! capture path.

use brain_core::body::Promoted;
use brain_core::event::Kind;
use brain_store::session::Session;
use brain_store::{StorePaths, promote};

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("brain-promote-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        let paths = StorePaths::under(p.clone());
        brain_store::store::create(&paths, 1_700_000_000_000).unwrap();
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
    // The path a real capture takes, which deliberately does not read anything.
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = brain_store::session::capture_body(text).unwrap();
    s.capture(&body).unwrap().slug
}

/// Read the log without opening a session.
///
/// Deliberately not through `Session::open`: that catches the store up, so the
/// act of checking what happened would change it. Reads go direct (§8.8).
fn events(paths: &StorePaths) -> Vec<brain_core::envelope::Envelope> {
    brain_store::log::Log::open_read(&paths.events)
        .unwrap()
        .all()
        .unwrap()
}

fn note_of(paths: &StorePaths, slug: &str) -> brain_core::fold::Note {
    brain_core::fold::fold(&events(paths))
        .notes
        .into_values()
        .find(|n| n.slug == slug)
        .expect("the note exists")
}

#[test]
fn a_sweep_promotes_what_clears_the_floor_and_leaves_the_rest() {
    let sc = Scratch::new("floor");
    let p = sc.paths();

    let confident = capture(&p, "dentist thursday 2pm");
    let unsure = capture(&p, "dentist thursday");
    let prose = capture(&p, "the march timeline slipped");

    let mut s = Session::open_for_capture(p.clone()).unwrap();
    let written = promote::sweep(&mut s).unwrap();
    drop(s);

    assert_eq!(written, 1, "expected exactly one promotion");
    assert!(note_of(&p, &confident).promotion.is_some());
    assert!(
        note_of(&p, &unsure).promotion.is_none(),
        "a reading under the floor was promoted"
    );
    assert!(note_of(&p, &prose).promotion.is_none());
}

#[test]
fn promotion_never_touches_the_captured_text() {
    // The guarantee that makes a wrong interpretation survivable (§3.1.3).
    let sc = Scratch::new("verbatim");
    let p = sc.paths();
    let text = "dentist thursday 2pm #health";
    let slug = capture(&p, text);

    let mut s = Session::open_for_capture(p.clone()).unwrap();
    promote::sweep(&mut s).unwrap();
    drop(s);

    let n = note_of(&p, &slug);
    assert_eq!(n.text.as_deref(), Some(text), "the text was rewritten");
    assert!(n.promotion.is_some());
}

#[test]
fn sweeping_twice_writes_nothing_the_second_time() {
    // A sweep that re-promoted everything it had already promoted would add an
    // event per note per run, forever.
    let sc = Scratch::new("idempotent");
    let p = sc.paths();
    capture(&p, "dentist thursday 2pm");

    let mut s = Session::open_for_capture(p.clone()).unwrap();
    assert_eq!(promote::sweep(&mut s).unwrap(), 1);
    assert_eq!(promote::sweep(&mut s).unwrap(), 0, "it promoted again");
}

#[test]
fn a_correction_survives_every_later_sweep() {
    // The one conflict rule that is not last-write-wins, seen from the log.
    let sc = Scratch::new("pinned");
    let p = sc.paths();
    let slug = capture(&p, "dentist thursday 2pm");

    let mut s = Session::open_for_capture(p.clone()).unwrap();
    promote::sweep(&mut s).unwrap();
    drop(s);

    promote::correct(&p, &slug, promote::task(false, Some("2026-08-14"))).unwrap();

    let mut s = Session::open_for_capture(p.clone()).unwrap();
    let written = promote::sweep(&mut s).unwrap();
    drop(s);
    assert_eq!(written, 0, "a sweep reparsed a corrected note");

    let n = note_of(&p, &slug);
    let promotion = n.promotion.expect("still promoted");
    assert!(promotion.pinned, "the correction was displaced");
    assert!(
        matches!(promotion.as_, Promoted::Task(_)),
        "the correction's type was replaced: {:?}",
        promotion.as_
    );
}

#[test]
fn a_capture_does_not_wait_for_any_of_this() {
    // §3.1: the overlay closes on Enter and parsing happens after the commit.
    // The check that matters is structural rather than timed — a capture must
    // not write a promotion, whatever the parser would have said about it.
    let sc = Scratch::new("no-wait");
    let p = sc.paths();
    capture(&p, "dentist thursday 2pm");

    let kinds: Vec<Kind> = events(&p).iter().map(|e| e.header.kind).collect();
    assert!(
        !kinds.contains(&Kind::PROMOTE),
        "capturing wrote a promotion on the capture path"
    );
}

#[test]
fn a_note_with_no_zone_is_read_in_utc_rather_than_locally() {
    // Reading it in whatever this machine is set to would make the same note
    // parse differently on two devices — the property the whole parse path is
    // arranged to avoid.
    let sc = Scratch::new("no-zone");
    let p = sc.paths();
    let slug = capture(&p, "vacation aug 1-14");

    let mut s = Session::open_for_capture(p.clone()).unwrap();
    promote::sweep(&mut s).unwrap();
    drop(s);

    // Whatever zone the capture recorded, an all-day range must be dates.
    //
    // The day is asserted and the year is derived, because the year depends on
    // when this runs: "aug 1-14" is the coming August, so a capture made in
    // September means next year — the same forward rule as "revisit in Q2".
    // This read `assert_eq!(start_date, "2026-08-01")` while the sweep took an
    // injected June clock, and hardcoding the year outlived the clock that made
    // it true.
    let note = note_of(&p, &slug);
    let captured = note.created.wall as i64;
    let utc = brain_time::Zone::new("UTC").unwrap();
    let today = brain_time::civil_date(captured, utc);
    let year = if (today.month, today.day) > (8, 1) {
        today.year + 1
    } else {
        today.year
    };

    match note.promotion.map(|p| p.as_) {
        Some(Promoted::Event(f)) => match f.when {
            brain_core::body::When::AllDay { start_date, .. } => {
                assert_eq!(start_date, format!("{year}-08-01"));
            }
            other => panic!("a range became {other:?}"),
        },
        other => panic!("expected an all-day event, got {other:?}"),
    }
}

#[test]
fn a_reparse_reads_a_note_again_rather_than_rescheduling_it() {
    // Found on a live store the first time a grammar bump swept one. "Meeting
    // at 14:00", captured yesterday, came back promoted to *today* — because
    // the sweep resolved every relative expression against its own clock rather
    // than against the capture. It would have moved again on every later bump.
    //
    // A sweep is a better reading of what somebody wrote, not a rescheduling of
    // it: "at 14:00" written on Tuesday means Tuesday however often it is read.
    //
    // The drift itself cannot be reproduced here, and that is worth stating
    // rather than papering over: it needs a capture stamped in the past, and
    // there is no way to make one — the HLC takes the real clock, which is what
    // makes the log trustworthy. So the property is pinned two ways instead.
    // First, and mainly, `sweep` no longer takes a clock at all: there is no
    // "now" for it to resolve against, so the failure is unrepresentable rather
    // than merely absent, and this test would not compile if the parameter came
    // back. Second, the promotion has to land on the capture's own day.
    let scratch = Scratch::new("reparse-time");
    let paths = scratch.paths();

    let mut s = Session::open(paths.clone()).unwrap();
    let body = brain_store::session::capture_body("meeting at 14:00").unwrap();
    let slug = s.capture(&body).unwrap().slug;

    // No clock passed. That is the assertion.
    promote::sweep(&mut s).unwrap();

    let tgt = s.state.resolve(&slug).unwrap().unwrap();
    let note = s.state.note(&tgt).unwrap().unwrap();
    let when = match s.state.promotion_of(&tgt).unwrap().map(|p| p.as_) {
        Some(Promoted::Event(e)) => e.when,
        other => panic!("expected an event, got {other:?}"),
    };

    match when {
        brain_core::body::When::Timed { start_ms, .. } => {
            let zone = brain_time::Zone::new(note.tz.as_deref().unwrap_or("UTC"))
                .unwrap_or_else(|| brain_time::Zone::new("UTC").unwrap());
            assert_eq!(
                format!("{}", brain_time::civil_date(start_ms, zone)),
                format!("{}", brain_time::civil_date(note.created.0 as i64, zone)),
                "the meeting did not land on the day it was captured"
            );
        }
        other => panic!("expected a timed event, got {other:?}"),
    }
}

#[test]
fn editing_the_text_reads_it_again() {
    // The milestone recorded this as done and it was not. Editing "dentist
    // thursday 2pm" into "dentist friday 3pm" left the promotion saying
    // Thursday at two — the note said one thing, the agenda showed another, and
    // nothing reparsed because nothing was watching the text.
    //
    // §3.1.4 keeps text and fields apart deliberately and shows both, so
    // *deliberate* drift is visible. Fields that silently failed to follow an
    // edit are not that.
    let sc = Scratch::new("reparse-edit");
    let p = sc.paths();
    let slug = capture(&p, "dentist thursday 2pm");

    let mut s = Session::open_for_capture(p.clone()).unwrap();
    promote::sweep(&mut s).unwrap();
    drop(s);

    let first = descriptor_of(&p, &slug);
    assert!(!first.is_empty(), "it never promoted at all");

    brain_store::mutate::edit(&p, &slug, "dentist friday 3pm").unwrap();
    let mut s = Session::open_for_capture(p.clone()).unwrap();
    let written = promote::sweep(&mut s).unwrap();
    drop(s);

    assert_eq!(written, 1, "an edited note was not read again");
    assert_ne!(
        descriptor_of(&p, &slug),
        first,
        "the fields still describe the text that was replaced"
    );
}

#[test]
fn reading_it_again_does_not_start_a_loop() {
    // `updated` moves only on a capture or an edit — a promotion is not a
    // version and must not touch it. If it did, every sweep would see a
    // promotion older than the text it had just written and reparse for ever.
    let sc = Scratch::new("reparse-settles");
    let p = sc.paths();
    let slug = capture(&p, "dentist thursday 2pm");
    brain_store::mutate::edit(&p, &slug, "dentist friday 3pm").unwrap();

    let mut s = Session::open_for_capture(p.clone()).unwrap();
    assert_eq!(
        promote::sweep(&mut s).unwrap(),
        1,
        "it did not read the note"
    );
    assert_eq!(
        promote::sweep(&mut s).unwrap(),
        0,
        "it read the same note twice, which is a sweep that never settles"
    );
}

#[test]
fn an_edit_does_not_displace_a_correction() {
    // §6 rates a reparse silently reverting somebody's correction as the
    // failure worth preventing above all others here — and an edit is exactly
    // the moment a reparse now happens, so the pin has to hold across one.
    let sc = Scratch::new("reparse-pinned");
    let p = sc.paths();
    let slug = capture(&p, "dentist thursday 2pm");

    promote::correct(&p, &slug, promote::task(false, Some("2026-12-01"))).unwrap();
    brain_store::mutate::edit(&p, &slug, "dentist monday 9am").unwrap();

    let mut s = Session::open_for_capture(p.clone()).unwrap();
    promote::sweep(&mut s).unwrap();
    drop(s);

    let s = Session::open_for_capture(p.clone()).unwrap();
    let tgt = s.state.resolve(&slug).unwrap().unwrap();
    let promotion = s.state.promotion_of(&tgt).unwrap().unwrap();
    assert!(promotion.pinned, "the correction lost its pin");
    assert_eq!(
        promotion.kind, "task",
        "an edit displaced a human correction"
    );
}

/// The one-line rendering of what a note currently is.
fn descriptor_of(p: &StorePaths, slug: &str) -> String {
    let s = Session::open_for_capture(p.clone()).unwrap();
    let tgt = s.state.resolve(slug).unwrap().unwrap();
    s.state
        .promotion_of(&tgt)
        .unwrap()
        .map(|x| x.descriptor)
        .unwrap_or_default()
}

#[test]
fn taking_the_time_out_of_a_note_takes_it_off_the_calendar() {
    // A reparse could promote and never demote. Editing "team sync every friday
    // at 11" down to prose left the series standing — an agenda entry whose own
    // text no longer says a time, which is the staleness the reparse exists to
    // prevent, arriving from the other direction.
    let sc = Scratch::new("demote");
    let p = sc.paths();
    let slug = capture(&p, "team sync every friday at 11");

    let mut s = Session::open_for_capture(p.clone()).unwrap();
    promote::sweep(&mut s).unwrap();
    drop(s);
    assert_eq!(kind_of(&p, &slug), "series", "it never promoted");

    brain_store::mutate::edit(&p, &slug, "the team sync question, still unresolved").unwrap();
    let mut s = Session::open_for_capture(p.clone()).unwrap();
    promote::sweep(&mut s).unwrap();
    drop(s);

    assert_eq!(
        kind_of(&p, &slug),
        "note",
        "the series outlived the text that described it"
    );
}

#[test]
fn an_ordinary_note_is_not_promoted_to_being_ordinary() {
    // The other half. A note that has never been anything stays nothing:
    // writing `note` for every non-temporal capture would put an event in the
    // log for each one, recording that the recognizer found what it usually
    // finds.
    let sc = Scratch::new("no-demote-churn");
    let p = sc.paths();
    let slug = capture(&p, "the argument against a second index");

    let mut s = Session::open_for_capture(p.clone()).unwrap();
    assert_eq!(promote::sweep(&mut s).unwrap(), 0, "prose was promoted");
    drop(s);

    let s = Session::open_for_capture(p.clone()).unwrap();
    let tgt = s.state.resolve(&slug).unwrap().unwrap();
    assert!(s.state.promotion_of(&tgt).unwrap().is_none());
}

/// What a note currently is, or the empty string.
fn kind_of(p: &StorePaths, slug: &str) -> String {
    let s = Session::open_for_capture(p.clone()).unwrap();
    let tgt = s.state.resolve(slug).unwrap().unwrap();
    s.state
        .promotion_of(&tgt)
        .unwrap()
        .map(|x| x.kind)
        .unwrap_or_default()
}
