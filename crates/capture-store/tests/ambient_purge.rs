//! Redaction takes the context the note was captured in.
//!
//! `purge_note` edits the projection in place rather than re-folding, so every
//! derived table has to be named in it. Its own comment records this class of
//! bug being found twice before — `mentions`, then `list_items` — and `ambient`
//! was missed both times.
//!
//! It holds the foreground application, the window title, the working
//! directory, the repository and the branch. §1.1's argument about the search
//! index applies unchanged: destroying the body while leaving a derived copy of
//! the same material is the stated purpose failing. A window title is often the
//! same material as the note beside it.

use capture_store::StorePaths;
use capture_store::session::Session;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "capture-ambient-{tag}-{}-{:?}",
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

fn destroy(paths: &StorePaths, slug: &str) {
    capture_store::mutate::redact(paths, slug).unwrap();
    let mut s = Session::open(paths.clone()).unwrap();
    assert!(s.drain_due_at(PAST_THE_WINDOW).unwrap().destroyed >= 1);
}

/// The repository this test is running in, as the capture recorded it.
///
/// Read back rather than hardcoded: the point is that whatever context was
/// stored is the context that must go.
fn repo_of(paths: &StorePaths, tgt: &capture_core::EventId) -> Option<String> {
    capture_store::projection::current(paths)
        .unwrap()
        .ambient_of(tgt)
        .unwrap()
        .into_iter()
        .find(|(k, _)| k == "repo")
        .map(|(_, v)| v)
}

#[test]
fn a_purged_note_keeps_no_context() {
    let sc = Scratch::new("gone");
    let p = sc.paths();
    let slug = capture(&p, "something with a context worth not keeping");
    capture_store::promote::settle(&p).unwrap();

    let tgt = capture_store::projection::current(&p)
        .unwrap()
        .resolve(&slug)
        .unwrap()
        .expect("the note is named");
    assert!(
        !capture_store::projection::current(&p)
            .unwrap()
            .ambient_of(&tgt)
            .unwrap()
            .is_empty(),
        "the capture recorded no context, so this proves nothing"
    );

    destroy(&p, &slug);

    assert!(
        capture_store::projection::current(&p)
            .unwrap()
            .ambient_of(&tgt)
            .unwrap()
            .is_empty(),
        "the context survived the destruction of the note"
    );
}

#[test]
fn a_destroyed_note_is_not_returned_by_a_context_query() {
    // `by_context` joins `notes` to `ambient` with no redaction filter, so
    // while the row survived the destroyed note went on being returned — with
    // its context attached.
    let sc = Scratch::new("query");
    let p = sc.paths();
    let slug = capture(&p, "written somewhere identifiable");
    capture_store::promote::settle(&p).unwrap();

    let tgt = capture_store::projection::current(&p)
        .unwrap()
        .resolve(&slug)
        .unwrap()
        .unwrap();
    let Some(repo) = repo_of(&p, &tgt) else {
        // Not running inside a repository, so there is nothing to query by.
        return;
    };

    let before = capture_store::projection::current(&p)
        .unwrap()
        .by_context("repo", &repo)
        .unwrap();
    assert_eq!(
        before.len(),
        1,
        "the note should match before it is destroyed"
    );

    destroy(&p, &slug);

    let after = capture_store::projection::current(&p)
        .unwrap()
        .by_context("repo", &repo)
        .unwrap();
    assert!(
        after.is_empty(),
        "a destroyed note is still returned by a context query"
    );
}

#[test]
fn another_notes_context_is_untouched() {
    let sc = Scratch::new("neighbour");
    let p = sc.paths();
    let doomed = capture(&p, "this one goes");
    let kept = capture(&p, "this one stays");
    capture_store::promote::settle(&p).unwrap();

    let kept_tgt = capture_store::projection::current(&p)
        .unwrap()
        .resolve(&kept)
        .unwrap()
        .unwrap();
    let before = capture_store::projection::current(&p)
        .unwrap()
        .ambient_of(&kept_tgt)
        .unwrap();

    destroy(&p, &doomed);

    assert_eq!(
        capture_store::projection::current(&p)
            .unwrap()
            .ambient_of(&kept_tgt)
            .unwrap(),
        before,
        "purging one note took another's context"
    );
}
