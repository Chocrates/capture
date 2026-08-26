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

/// Monday 1 June 2026, 09:00 in Denver.
const NOW: i64 = 1_780_326_000_000;

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
    let mut s = Session::open(paths.clone()).unwrap();
    let body = brain_store::session::capture_body(text).unwrap();
    s.capture(&body).unwrap().slug
}

fn note_of(paths: &StorePaths, slug: &str) -> brain_core::fold::Note {
    let s = Session::open(paths.clone()).unwrap();
    let all = s.log.all().unwrap();
    brain_core::fold::fold(&all)
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

    let mut s = Session::open(p.clone()).unwrap();
    let written = promote::sweep(&mut s, NOW).unwrap();
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

    let mut s = Session::open(p.clone()).unwrap();
    promote::sweep(&mut s, NOW).unwrap();
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

    let mut s = Session::open(p.clone()).unwrap();
    assert_eq!(promote::sweep(&mut s, NOW).unwrap(), 1);
    assert_eq!(promote::sweep(&mut s, NOW).unwrap(), 0, "it promoted again");
}

#[test]
fn a_correction_survives_every_later_sweep() {
    // The one conflict rule that is not last-write-wins, seen from the log.
    let sc = Scratch::new("pinned");
    let p = sc.paths();
    let slug = capture(&p, "dentist thursday 2pm");

    let mut s = Session::open(p.clone()).unwrap();
    promote::sweep(&mut s, NOW).unwrap();
    drop(s);

    promote::correct(&p, &slug, promote::task(false, Some("2026-08-14"))).unwrap();

    let mut s = Session::open(p.clone()).unwrap();
    let written = promote::sweep(&mut s, NOW).unwrap();
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

    let s = Session::open(p.clone()).unwrap();
    let kinds: Vec<Kind> = s.log.all().unwrap().iter().map(|e| e.header.kind).collect();
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

    let mut s = Session::open(p.clone()).unwrap();
    promote::sweep(&mut s, NOW).unwrap();
    drop(s);

    // Whatever zone the capture recorded, an all-day range must be dates.
    match note_of(&p, &slug).promotion.map(|p| p.as_) {
        Some(Promoted::Event(f)) => match f.when {
            brain_core::body::When::AllDay { start_date, .. } => {
                assert_eq!(start_date, "2026-08-01");
            }
            other => panic!("a range became {other:?}"),
        },
        other => panic!("expected an all-day event, got {other:?}"),
    }
}
