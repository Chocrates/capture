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

#[test]
fn a_hash_in_a_capture_reaches_its_page() {
    // The first version of `mentions` left `#` out, on the grounds that tags
    // have their own event kind. True, and it meant typing `#reading` in a
    // capture did nothing whatsoever — the one organising axis somebody writes
    // by hand was the only one that never appeared anywhere.
    let sc = Scratch::new("hash");
    let p = sc.paths();
    capture(&p, "notes on the paper #reading");

    let s = Session::open(p.clone()).unwrap();
    assert_eq!(s.state.tagged("reading", u64::MAX).unwrap().len(), 1);
}

#[test]
fn a_written_tag_and_an_added_one_are_one_page_and_one_row() {
    // Different facts about a note — one is part of what it says, the other an
    // OR-Set membership that can be retracted — and both belong on the page.
    // A note carrying both must still appear once.
    let sc = Scratch::new("hash-both");
    let p = sc.paths();
    let written = capture(&p, "notes on the paper #reading");
    let added = capture(&p, "a plain note with no sigil");
    brain_store::mutate::tag(&p, &added, "reading").unwrap();
    brain_store::mutate::tag(&p, &written, "reading").unwrap();

    let s = Session::open(p.clone()).unwrap();
    let rows = s.state.tagged("reading", u64::MAX).unwrap();
    assert_eq!(
        rows.len(),
        2,
        "a note carrying both was counted twice: {rows:?}"
    );
}

// ---------------------------------------------------------------------------
// What the wiki's sidebar is built from.
// ---------------------------------------------------------------------------

#[test]
fn the_pages_are_the_notes_somebody_named() {
    // An alias is the one act in this system that says "this is a thing I will
    // come back to". Listing every note instead would be the timeline with
    // worse ordering.
    let sc = Scratch::new("named");
    let p = sc.paths();
    let a = capture(&p, "the acme plan in full");
    capture(&p, "an ordinary capture nobody named");
    brain_store::mutate::set_alias(&p, &a, Some("acme-plan")).unwrap();

    let s = Session::open(p.clone()).unwrap();
    let named = s.state.named().unwrap();
    assert_eq!(named.len(), 1, "{named:?}");
    assert_eq!(named[0].alias, "acme-plan");
}

#[test]
fn the_names_are_ordered_by_use() {
    // A sidebar is read top-down and the name you want is usually one you use.
    // Alphabetical alone puts whoever is called Aaron permanently above
    // everyone you actually work with.
    let sc = Scratch::new("distinct");
    let p = sc.paths();
    capture(&p, "kickoff with @sarah and @aaron");
    capture(&p, "@sarah again");
    capture(&p, "and once more with @sarah");

    let s = Session::open(p.clone()).unwrap();
    let people = s.state.distinct_mentions('@').unwrap();
    assert_eq!(people, [("sarah".into(), 3), ("aaron".into(), 1)]);
}

#[test]
fn a_tag_is_counted_once_however_it_arrived() {
    // Tags come from two places — written in the text, or added by a `tag`
    // event. A note carrying both must count once, or the sidebar reports more
    // notes than the page it opens will show.
    let sc = Scratch::new("distinct-tags");
    let p = sc.paths();
    let both = capture(&p, "notes on the paper #reading");
    brain_store::mutate::tag(&p, &both, "reading").unwrap();

    let s = Session::open(p.clone()).unwrap();
    assert_eq!(
        s.state.distinct_mentions('#').unwrap(),
        [("reading".into(), 1)]
    );
}

#[test]
fn a_muted_note_does_not_put_a_name_in_the_sidebar() {
    // §2.6 again: muting is the cheap way to stop something resurfacing, and a
    // name that only a muted note mentions would keep appearing.
    let sc = Scratch::new("distinct-muted");
    let p = sc.paths();
    let slug = capture(&p, "the thing with @quiet");
    brain_store::mutate::mute_note(&p, &slug).unwrap();

    let s = Session::open(p.clone()).unwrap();
    assert!(s.state.distinct_mentions('@').unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Nesting. §3.7 argues against filing and the argument is not withdrawn; what
// these pin is that the cost of a wrong parent stays inside one sidebar.
// ---------------------------------------------------------------------------

#[test]
fn a_page_sits_under_another_and_comes_back_out() {
    let sc = Scratch::new("nest");
    let p = sc.paths();
    let parent = capture(&p, "the acme migration");
    let child = capture(&p, "budget for acme");
    brain_store::mutate::set_alias(&p, &parent, Some("acme")).unwrap();
    brain_store::mutate::set_alias(&p, &child, Some("acme-budget")).unwrap();

    brain_store::mutate::set_parent(&p, "acme-budget", Some("acme")).unwrap();
    let s = Session::open(p.clone()).unwrap();
    let named = s.state.named().unwrap();
    let row = named.iter().find(|n| n.alias == "acme-budget").unwrap();
    assert_eq!(row.parent.as_deref(), Some(parent.as_str()));

    let tgt = s.state.resolve("acme").unwrap().unwrap();
    assert_eq!(s.state.children_of(&tgt).unwrap().len(), 1);
    drop(s);

    brain_store::mutate::set_parent(&p, "acme-budget", None).unwrap();
    let s = Session::open(p.clone()).unwrap();
    let named = s.state.named().unwrap();
    let row = named.iter().find(|n| n.alias == "acme-budget").unwrap();
    assert_eq!(row.parent, None, "it could not be taken back out");
}

#[test]
fn filing_a_page_removes_it_from_nothing() {
    // The whole answer to §3.7's objection, and the reason this was allowed to
    // exist at all: a page in the wrong branch is misplaced in one sidebar and
    // is still everywhere else it was. "I filed it and now it is gone" is the
    // failure being avoided, and it cannot happen if filing removes nothing.
    let sc = Scratch::new("nest-visible");
    let p = sc.paths();
    let parent = capture(&p, "the acme migration");
    let child = capture(&p, "budget for acme, ask @sarah #money");
    brain_store::mutate::set_alias(&p, &parent, Some("acme")).unwrap();
    brain_store::mutate::set_parent(&p, &child, Some("acme")).unwrap();

    let s = Session::open(p.clone()).unwrap();
    let tgt = s.state.resolve(&child).unwrap().unwrap();

    assert!(
        s.state.timeline(10).unwrap().iter().any(|n| n.tgt == tgt),
        "filing took it out of the timeline"
    );
    assert!(
        s.state.search("budget", u64::MAX).unwrap().contains(&tgt),
        "filing took it out of search"
    );
    assert_eq!(
        s.state.mentioning('@', "sarah", u64::MAX).unwrap().len(),
        1,
        "filing took it off an entity page"
    );
    assert_eq!(
        s.state.tagged("money", u64::MAX).unwrap().len(),
        1,
        "filing took it off a tag page"
    );
}

#[test]
fn a_loop_you_can_see_is_refused_where_you_make_it() {
    // The concurrent case cannot be refused — two devices each write half of it
    // and neither is wrong at the time, so the fold breaks those afterwards.
    // This one is a single mistake with a single author, and it can be
    // explained instead.
    let sc = Scratch::new("nest-loop");
    let p = sc.paths();
    let a = capture(&p, "the acme migration");
    let b = capture(&p, "budget for acme");
    brain_store::mutate::set_parent(&p, &b, Some(&a)).unwrap();

    let e = brain_store::mutate::set_parent(&p, &a, Some(&b)).unwrap_err();
    assert!(e.to_string().contains("loop"), "unhelpful refusal: {e}");

    let e = brain_store::mutate::set_parent(&p, &a, Some(&a)).unwrap_err();
    assert!(
        e.to_string().contains("under itself"),
        "unhelpful refusal: {e}"
    );
}

#[test]
fn every_note_query_reads_the_columns_it_selected() {
    // Adding `parent` broke four queries at once, because each wrote its column
    // list out by hand instead of using the shared one — "Invalid column index:
    // 14" from a store that was perfectly fine, and nothing caught it, because
    // a hand-written list still compiles.
    //
    // Exercising them is the check. Each of these builds a `NoteRow`, and each
    // would fail on the first row if its list and `note_row` disagreed.
    let sc = Scratch::new("columns");
    let p = sc.paths();
    let slug = capture(&p, "a note to read every way there is");

    let s = Session::open(p.clone()).unwrap();
    let tgt = s.state.resolve(&slug).unwrap().unwrap();

    s.state.note(&tgt).unwrap().expect("note()");
    s.state.timeline(10).unwrap();
    s.state.needing_parse(u32::MAX, 10).unwrap();
    s.state.by_context("repo", "second-brain").unwrap();
    s.state.review_never_engaged(i64::MAX, 5).unwrap();
    s.state.mentioning('@', "nobody", u64::MAX).unwrap();
    s.state.tagged("nothing", u64::MAX).unwrap();
}

#[test]
fn a_projection_from_an_older_build_can_still_be_opened() {
    // The rule this pins: **the schema may add tables, never columns.**
    //
    // Rebuild-not-migrate (§1.3) depends on an old projection being openable
    // long enough to be found stale — opening is how a rebuild starts. Adding
    // `parent` as a column on `notes` broke exactly that: `CREATE TABLE IF NOT
    // EXISTS notes` does nothing to a table already there, so the column never
    // appeared and the index over it failed, and every write path on every
    // existing store failed with it. The fix that would have healed them could
    // not run, because running it required opening the thing that would not
    // open.
    //
    // Simulated the only honest way: a database holding the `notes` table as an
    // earlier build wrote it, and nothing else.
    let dir = std::env::temp_dir().join(format!("brain-oldschema-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("state.0.db");

    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE meta (k TEXT PRIMARY KEY, v TEXT NOT NULL) STRICT;
             INSERT INTO meta (k, v) VALUES ('schema', '1');
             CREATE TABLE notes (
               tgt             BLOB PRIMARY KEY,
               slug            TEXT NOT NULL UNIQUE,
               alias           TEXT UNIQUE,
               alias_wall      INTEGER,
               alias_counter   INTEGER,
               text            TEXT,
               head            BLOB NOT NULL,
               created_wall    INTEGER NOT NULL,
               created_counter INTEGER NOT NULL,
               updated_wall    INTEGER NOT NULL,
               updated_counter INTEGER NOT NULL,
               tz              TEXT,
               pinned          INTEGER NOT NULL DEFAULT 0,
               muted_until     INTEGER,
               redacted_at     INTEGER,
               credential      INTEGER NOT NULL DEFAULT 0
             ) STRICT;",
        )
        .unwrap();
    }

    let store = brain_store::projection::Store::open(&path)
        .expect("an older projection must still open, or it can never be rebuilt");
    assert!(
        store.is_stale().unwrap(),
        "it opened but did not report itself stale, so nothing would rebuild it"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
