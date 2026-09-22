//! The projection is derived, and something has to notice when it isn't.
//!
//! §8.8 says the projection can be deleted at any time and §1.3 says the
//! migration strategy is "throw it away and compute it again". Both are true
//! only if something rebuilds, and for a long time nothing did: `is_stale`
//! asked whether the projection had the *shape* this build expects and never
//! whether it held what the log holds.
//!
//! Those come apart in ordinary ways. A sync round ingests forty events and
//! the connection drops before the fold — the events are committed, the log's
//! heads have moved, and anti-entropy will never re-offer them because they
//! are no longer wanted. A process is killed between the append and the fold.
//! The projection is deleted by somebody following the documentation.
//!
//! In every one of those the captures exist, no surface shows them, and
//! nothing says anything. These tests are the thing that notices.

use capture_store::StorePaths;
use capture_store::session::Session;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!("capture-recon-{name}-{}", std::process::id()));
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

fn capture(paths: &StorePaths, text: &str) {
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = capture_store::session::capture_body(text).unwrap();
    s.capture(&body).unwrap();
}

fn notes(paths: &StorePaths) -> Vec<String> {
    capture_store::projection::current(paths)
        .unwrap()
        .timeline(50)
        .unwrap()
        .into_iter()
        .filter_map(|n| n.text)
        .collect()
}

/// Deleting the projection is survivable, which is what the documentation says.
#[test]
fn a_deleted_projection_comes_back() {
    let scratch = Scratch::new("deleted");
    let paths = scratch.paths();

    capture(&paths, "one");
    capture(&paths, "two");
    let _ = Session::open(paths.clone()).unwrap();
    assert_eq!(notes(&paths).len(), 2, "both captured");

    // What §8.8 says is safe at any time.
    for f in std::fs::read_dir(&scratch.0).unwrap() {
        let f = f.unwrap().path();
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with("state.") {
            let _ = std::fs::remove_file(&f);
        }
    }

    // Opening a session is what notices. Before this, a fresh empty
    // projection was created and stamped with the current schema, so
    // `is_stale` answered false and the whole archive was invisible.
    let _ = Session::open(paths.clone()).unwrap();

    let back = notes(&paths);
    assert_eq!(
        back.len(),
        2,
        "the archive is back, not silently empty: {back:?}"
    );
    assert!(back.iter().any(|t| t == "one"));
    assert!(back.iter().any(|t| t == "two"));
}

/// The sync case: events committed to the log that the projection never saw.
///
/// This is the one with no signal at all — the round ingests, the connection
/// drops before `settle`, the heads have moved so anti-entropy will not offer
/// them again, and both devices correctly report converged.
#[test]
fn events_in_the_log_but_not_the_fold_are_found() {
    let scratch = Scratch::new("unfolded");
    let paths = scratch.paths();

    capture(&paths, "before");
    let _ = Session::open(paths.clone()).unwrap();
    assert_eq!(notes(&paths).len(), 1);

    // Roll the projection back to a generation that never saw the second
    // capture, which is what an interrupted fold leaves behind.
    capture(&paths, "arrived by sync");
    let ops = capture_store::ops::Ops::open(&paths.ops).unwrap();
    let current = ops.generation().unwrap();
    if current > 0 {
        ops.set_generation(current - 1).unwrap();
    }

    let _ = Session::open(paths.clone()).unwrap();

    let back = notes(&paths);
    assert!(
        back.iter().any(|t| t == "arrived by sync"),
        "an event in the log that the fold missed is found and folded: {back:?}",
    );
}

/// And the ordinary case stays cheap.
///
/// A rebuild on every open would be correct and unusable — the check has to
/// be a comparison of heads, not a fold.
#[test]
fn an_up_to_date_projection_is_not_rebuilt() {
    let scratch = Scratch::new("current");
    let paths = scratch.paths();

    capture(&paths, "one");
    let _ = Session::open(paths.clone()).unwrap();

    let ops = capture_store::ops::Ops::open_read(&paths.ops).unwrap();
    let before = ops.generation().unwrap();

    for _ in 0..3 {
        let _ = Session::open(paths.clone()).unwrap();
    }

    let after = capture_store::ops::Ops::open_read(&paths.ops)
        .unwrap()
        .generation()
        .unwrap();
    assert_eq!(
        before,
        after,
        "opening a session three times rebuilt the projection {} times",
        after - before,
    );
}

/// A drain that commits nothing still catches the projection up.
///
/// The case with no signal and no second chance. `commit_body` catches *any*
/// error from a capture — including one raised after `log.append` has already
/// committed — spools the payload and reports "Saved." The body is in the log,
/// and the fold never ran.
///
/// The next drain then makes it permanent rather than fixing it: `holds_body`
/// is true, so the record is counted as already held and skipped without being
/// authored. Nothing is appended, so nothing is folded, and the spool is
/// cleared on the way out. The capture exists in the log, in no reader's view,
/// and in no queue that would ever look at it again — a note somebody wrote,
/// was told was saved, and cannot find.
///
/// Exercised through `open_for_capture`, which is where it happens: the capture
/// path deliberately skips the catch-up that every other open performs, because
/// §3.1 says the gesture must never wait for one.
#[test]
fn a_drain_that_commits_nothing_still_catches_the_projection_up() {
    let scratch = Scratch::new("already-held");
    let paths = scratch.paths();

    capture(&paths, "the one that was already saved");
    let _ = Session::open(paths.clone()).unwrap();
    assert_eq!(notes(&paths).len(), 1, "captured to begin with");

    // The payload exactly as the log holds it, which is what the spool holds
    // after a capture that failed after its append.
    let body = {
        let log = capture_store::log::Log::open_read(&paths.events).unwrap();
        log.all()
            .unwrap()
            .into_iter()
            .find_map(|e| e.body)
            .expect("the capture is in the log")
    };
    capture_store::spool::push(&paths.spool, &body).unwrap();

    // And the projection as that capture left it: the event committed, the
    // fold never reached. Removing it is the same starting state a fold that
    // never ran leaves — an empty projection whose heads sit below the log's.
    for f in std::fs::read_dir(&scratch.0).unwrap() {
        let f = f.unwrap().path();
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with("state.") {
            let _ = std::fs::remove_file(&f);
        }
    }

    let mut session = Session::open_for_capture(paths.clone()).unwrap();
    let committed = session.drain_spool().unwrap();
    drop(session);

    assert_eq!(committed, 0, "the log already held it; nothing to commit");
    assert!(
        !capture_store::spool::in_use(&paths.spool).unwrap(),
        "the spool is cleared once the log holds everything in it"
    );

    let back = notes(&paths);
    assert!(
        back.iter().any(|t| t == "the one that was already saved"),
        "a capture the drain skipped is still a capture somebody can find: {back:?}"
    );
}
