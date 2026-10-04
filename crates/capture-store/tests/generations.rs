//! Superseded projection generations do not accumulate.
//!
//! A rebuild writes the new generation beside the current one and moves the
//! pointer afterwards, which is right — readers keep the file they already
//! opened, and SQLite's sidecars are named from the path. Nothing ever came
//! back for the file it replaced.
//!
//! `discard_generation` was written to remove one and was called by nothing,
//! which `tools/unwired.py` found once it stopped counting a function's own
//! inline test module as a caller. The evidence was on a real archive holding
//! zero notes: `state.0.db` and `state.1.db`, both 320 KB, two minutes apart,
//! a fortnight old.
//!
//! §1.3 makes "throw it away and compute it again" the whole migration
//! strategy, so this is one full copy of the archive's derived state per
//! schema version, kept for ever, on every device.

use capture_store::StorePaths;
use capture_store::projection::generations;
use capture_store::session::Session;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "capture-generations-{tag}-{}-{:?}",
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

fn capture(paths: &StorePaths, text: &str) {
    let mut s = Session::open_for_capture(paths.clone()).unwrap();
    let body = capture_store::session::capture_body(text).unwrap();
    s.capture(&body).unwrap();
}

#[test]
fn rebuilding_repeatedly_does_not_accumulate_generations() {
    let sc = Scratch::new("accumulate");
    let p = sc.paths();
    capture(&p, "a note to give the projection something to hold");

    // Four rebuilds. Before this change every one of them left a file.
    for _ in 0..4 {
        let mut s = Session::open(p.clone()).unwrap();
        s.rebuild_projection().unwrap();
    }
    // Opening is where the tidy runs, so the last rebuild's predecessor is
    // still there until something opens afterwards.
    drop(Session::open(p.clone()).unwrap());

    let left = generations(&p.state).unwrap();
    assert!(
        left.len() <= 2,
        "a rebuild left its predecessor behind for ever: {left:?}"
    );
}

#[test]
fn the_current_generation_and_one_spare_survive() {
    // By number rather than by count, because "two files" would also be
    // satisfied by keeping the wrong two.
    let sc = Scratch::new("spare");
    let p = sc.paths();
    capture(&p, "a note");

    let mut current = 0;
    for _ in 0..3 {
        let mut s = Session::open(p.clone()).unwrap();
        current = s.rebuild_projection().unwrap();
    }
    drop(Session::open(p.clone()).unwrap());

    let left = generations(&p.state).unwrap();
    assert!(
        left.contains(&current),
        "the current generation was removed: {left:?} without {current}"
    );
    assert!(
        left.contains(&(current - 1)),
        "the spare was removed, which is what makes a read safe: {left:?}"
    );
    for g in &left {
        assert!(
            *g >= current - 1,
            "{g} is older than the spare and survived"
        );
    }
}

#[test]
fn a_store_that_has_never_been_rebuilt_keeps_what_it_has() {
    // Nothing to tidy is the ordinary case, and it must not remove the one
    // generation there is.
    let sc = Scratch::new("fresh");
    let p = sc.paths();
    capture(&p, "a note");
    drop(Session::open(p.clone()).unwrap());

    let left = generations(&p.state).unwrap();
    assert!(!left.is_empty(), "the only generation was removed");
}

#[test]
fn listing_ignores_anything_that_is_not_a_generation() {
    // Sidecars share the directory and the prefix, and a directory somebody
    // else is using must not be treated as ours to delete from.
    let sc = Scratch::new("listing");
    let p = sc.paths();
    capture(&p, "a note");
    let dir = p.state.parent().unwrap().to_path_buf();

    for name in [
        "state.9.db-wal",
        "state.9.db-shm",
        "state.notanumber.db",
        "notes.db",
    ] {
        std::fs::write(dir.join(name), b"x").unwrap();
    }

    let left = generations(&p.state).unwrap();
    assert!(
        !left.contains(&9),
        "a sidecar was read as a generation: {left:?}"
    );
    for name in ["state.9.db-wal", "state.notanumber.db", "notes.db"] {
        assert!(
            dir.join(name).exists(),
            "{name} was removed and is not ours"
        );
    }
}
