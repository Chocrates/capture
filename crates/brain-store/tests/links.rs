//! The wiki layer: names, and completing them as you type.
//!
//! The resolution rules are tested in `brain-core`, where the fold does them.
//! What is tested here is the query a surface asks while somebody is writing a
//! link — which is the only part that touches the projection.

use brain_store::StorePaths;
use brain_store::session::Session;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("brain-links-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        brain_store::store::create(&StorePaths::under(p.clone()), 1_700_000_000_000).unwrap();
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
    let body = brain_store::session::capture_body(text).unwrap();
    s.capture(&body).unwrap().slug
}

#[test]
fn a_prefix_offers_both_slugs_and_aliases() {
    // They share one namespace and one uniqueness constraint (§4.3). A
    // completion list offering only slugs would hide exactly the names somebody
    // chose for being memorable, which is the reason aliases exist.
    let sc = Scratch::new("both");
    let p = sc.paths();
    let slug = capture(&p, "acme migration kickoff");
    brain_store::mutate::set_alias(&p, &slug, Some("acme-plan")).unwrap();

    let s = Session::open(p.clone()).unwrap();
    let names = s.state.names_like("acme", 10).unwrap();
    assert!(names.iter().any(|n| n == "acme-plan"), "{names:?}");
    assert!(
        names.iter().any(|n| n.starts_with("acme-migration")),
        "{names:?}"
    );
}

#[test]
fn a_wildcard_in_a_name_matches_only_itself() {
    // A slug is kebab-case and cannot contain one, but an alias is whatever
    // somebody typed. Unescaped, a `%` would match every note in the store —
    // so the completion list for `100%` would offer the entire archive.
    let sc = Scratch::new("wildcard");
    let p = sc.paths();
    let a = capture(&p, "first note here");
    let b = capture(&p, "second note here");
    brain_store::mutate::set_alias(&p, &a, Some("100%-done")).unwrap();
    brain_store::mutate::set_alias(&p, &b, Some("something-else")).unwrap();

    let s = Session::open(p.clone()).unwrap();
    let names = s.state.names_like("100%", 10).unwrap();
    assert_eq!(names, ["100%-done"], "a wildcard escaped into a match-all");

    // The same for the single-character wildcard.
    assert!(
        s.state.names_like("10_", 10).unwrap().is_empty(),
        "an underscore matched a character it should have been escaped past"
    );
}

#[test]
fn a_destroyed_note_is_not_offered_to_link_to() {
    // A redacted note still has a slug. Offering it invites writing a link to
    // content that is gone — §1.1 keeps the tombstone visible in the timeline,
    // which is where an absence is explained, not where a new reference to it
    // should be created.
    let sc = Scratch::new("redacted");
    let p = sc.paths();
    let slug = capture(&p, "sensitive thing to destroy");

    let s = Session::open(p.clone()).unwrap();
    assert_eq!(s.state.names_like("sensitive", 10).unwrap().len(), 1);
    drop(s);

    brain_store::mutate::redact(&p, &slug).unwrap();

    // The queue is not the destruction: §1.1 keeps a cancellable window, so
    // nothing is gone yet and the ordinary drain will not touch it for hours.
    // Carried out here directly, at a time far enough ahead that it is due.
    let mut s = Session::open(p.clone()).unwrap();
    let ops = brain_store::ops::Ops::open(&p.ops).unwrap();
    let queue = s.state.redact_queue().unwrap();
    let work = brain_store::due::due(&queue, u64::MAX / 2);
    assert_eq!(work.len(), 1, "nothing was queued to destroy");
    for w in work {
        if let brain_store::due::Work::Destroy { redact, target } = w {
            brain_store::purge::execute(
                &mut s.log,
                &mut s.state,
                &ops,
                &redact,
                &target,
                u64::MAX / 2,
            )
            .unwrap();
        }
    }

    assert!(
        s.state.names_like("sensitive", 10).unwrap().is_empty(),
        "a destroyed note was offered as a link target"
    );
}

// ---------------------------------------------------------------------------
// Entity pages. §4.3: a page exists for every person and project immediately,
// which is what makes the layer usable on day one rather than after curation.
// ---------------------------------------------------------------------------

#[test]
fn a_page_exists_for_a_name_the_moment_it_is_written() {
    // Nothing is created and nothing is maintained. The note names somebody and
    // the page is the query.
    let sc = Scratch::new("mention");
    let p = sc.paths();
    capture(&p, "kickoff with @sarah on ^acme-migration");
    capture(&p, "@sarah is away next week");
    capture(&p, "nothing to do with anyone");

    let s = Session::open(p.clone()).unwrap();
    assert_eq!(s.state.mentioning('@', "sarah", u64::MAX).unwrap().len(), 2);
    assert_eq!(
        s.state
            .mentioning('^', "acme-migration", u64::MAX)
            .unwrap()
            .len(),
        1
    );
    assert!(
        s.state
            .mentioning('@', "nobody", u64::MAX)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn an_edit_moves_a_note_between_pages() {
    // A mention is part of what the note says, unlike a tag, which is added and
    // retracted by its own events. Editing the text out has to take the note
    // off the page — otherwise the page reports something the note no longer
    // says, which is the shape of lying this system exists to avoid.
    let sc = Scratch::new("mention-edit");
    let p = sc.paths();
    let slug = capture(&p, "lunch with @sarah");

    let s = Session::open(p.clone()).unwrap();
    assert_eq!(s.state.mentioning('@', "sarah", u64::MAX).unwrap().len(), 1);
    drop(s);

    brain_store::mutate::edit(&p, &slug, "lunch with @sam").unwrap();

    let s = Session::open(p.clone()).unwrap();
    assert!(
        s.state
            .mentioning('@', "sarah", u64::MAX)
            .unwrap()
            .is_empty(),
        "a page still names a note whose text no longer names it"
    );
    assert_eq!(s.state.mentioning('@', "sam", u64::MAX).unwrap().len(), 1);
}

#[test]
fn a_muted_note_stays_off_the_page() {
    // §2.6 makes muting the cheap way to stop something resurfacing, and a page
    // that kept showing it would defeat exactly that.
    let sc = Scratch::new("mention-muted");
    let p = sc.paths();
    let slug = capture(&p, "the thing with @sarah I do not want to see");

    brain_store::mutate::mute_note(&p, &slug).unwrap();

    let s = Session::open(p.clone()).unwrap();
    assert!(
        s.state
            .mentioning('@', "sarah", 1_700_000_000_000)
            .unwrap()
            .is_empty(),
        "a muted note appeared on an entity page"
    );
}
