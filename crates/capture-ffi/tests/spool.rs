//! Capture on the phone when the store will not take it.
//!
//! # Why this exists separately from the desktop's spool test
//!
//! The promise is the same — a capture the interface accepted is never a lie —
//! but the platform is not. Android kills applications routinely and without
//! warning, and the store can be unwritable for reasons the desktop rarely
//! sees: a device encrypted and not yet unlocked after boot, storage the system
//! has taken away under pressure, a process killed between opening the log and
//! writing to it. §3.4 calls this "more load-bearing here" for that reason.
//!
//! The desktop's version drives the command line. This one drives `Archive`,
//! because that is the object the phone holds, and a promise kept everywhere
//! except through the binding is not kept on the device that needs it most.
//!
//! # Why a read-only database rather than a full disk
//!
//! Filling a real disk is not something a test suite should do, and the failure
//! being modelled is not specifically "no space" — it is "the store refuses to
//! be written". Revoking write permission produces exactly that, at the same
//! layer.
//!
//! The `-shm` and `-wal` files have to be revoked too. SQLite writes to the
//! shared-memory index on any WAL connection, so leaving them writable leaves a
//! path that still works and the test passes without the failure ever having
//! happened.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use capture_ffi::{Archive, Captured};

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "capture-ffi-spool-{name}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// Every file SQLite might still find a way to write through.
fn log_files(root: &Path) -> Vec<PathBuf> {
    let events = root.join("events.db");
    vec![
        events.clone(),
        events.with_extension("db-shm"),
        events.with_extension("db-wal"),
    ]
}

fn set_writable(root: &Path, writable: bool) {
    for f in log_files(root) {
        if let Ok(meta) = std::fs::metadata(&f) {
            let mut p = meta.permissions();
            p.set_mode(if writable { 0o644 } else { 0o444 });
            let _ = std::fs::set_permissions(&f, p);
        }
    }
}

/// Restore write permission even if an assertion unwound, or the directory
/// cannot be removed and the next run inherits a read-only store.
struct Restore(PathBuf);

impl Drop for Restore {
    fn drop(&mut self) {
        set_writable(&self.0, true);
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_capture_survives_a_store_that_will_not_be_written() {
    let root = scratch("survives");
    let a = Archive::open(root.to_string_lossy().to_string()).unwrap();
    let _restore = Restore(root.clone());

    // Something ordinary first, so the failure below is the store being taken
    // away rather than it never having worked.
    assert!(matches!(
        a.capture("before the trouble".into()).unwrap(),
        Captured::Stored { .. }
    ));

    set_writable(&root, false);

    // The gesture still succeeds. This is the whole promise: the store being
    // unavailable is not the user's problem to solve at the moment they are
    // trying to write something down.
    let out = a
        .capture("thought had while the store was gone".into())
        .unwrap();
    assert!(
        matches!(out, Captured::Spooled),
        "a capture that cannot be stored is spooled, not lost: {out:?}",
    );

    // And it is said out loud rather than left for someone to discover. A
    // spool that fills silently is indistinguishable from a system that is
    // working.
    let said = a.health().unwrap();
    assert!(
        said.iter().any(|c| c.text.contains("spool")),
        "the health surface reports the spool: {said:?}",
    );

    set_writable(&root, true);

    // Opening a session drains it, which is what every resume on the phone
    // does. Nothing has to be asked for.
    a.settle().unwrap();

    let texts: Vec<_> = a
        .timeline(50)
        .unwrap()
        .into_iter()
        .filter_map(|n| n.text)
        .collect();
    assert!(
        texts
            .iter()
            .any(|t| t == "thought had while the store was gone"),
        "the spooled capture landed in the log once the store came back: {texts:?}",
    );
    assert!(
        texts.iter().any(|t| t == "before the trouble"),
        "and nothing else was lost doing it: {texts:?}",
    );

    // The condition clears itself. One that stayed after the cause was gone
    // would train the reader to ignore the surface it appears on (§4.5).
    let after = a.health().unwrap();
    assert!(
        !after.iter().any(|c| c.text.contains("spool")),
        "the spool is no longer reported once it is empty: {after:?}",
    );
}

#[test]
fn the_spooled_capture_is_stored_exactly_as_given() {
    let root = scratch("verbatim");
    let a = Archive::open(root.to_string_lossy().to_string()).unwrap();
    let _restore = Restore(root.clone());

    set_writable(&root, false);

    // A list, because the recognizer runs on the way out of the spool rather
    // than on the way in — and a capture that arrived by the slow path has to
    // be read the same as one that did not.
    let text = "#groceries: milk, eggs, bread";
    assert!(matches!(a.capture(text.into()).unwrap(), Captured::Spooled));

    set_writable(&root, true);
    a.settle().unwrap();

    let note = a
        .timeline(50)
        .unwrap()
        .into_iter()
        .find(|n| n.text.as_deref() == Some(text))
        .expect("the spooled capture is in the timeline, unchanged");

    assert_eq!(
        note.list_tag.as_deref(),
        Some("groceries"),
        "and is recognised on arrival like any other capture",
    );

    let items: Vec<_> = a
        .list("groceries".into())
        .unwrap()
        .into_iter()
        .map(|l| l.text)
        .collect();
    assert_eq!(items, ["milk", "eggs", "bread"]);
}
