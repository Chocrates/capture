//! Capture when the store will not take it.
//!
//! # The promise being kept
//!
//! A capture the interface accepted is never a lie. The store being unavailable
//! is not the user's problem to solve at the moment they are trying to write
//! something down — so the capture goes somewhere durable and lands in the log
//! when the store recovers.
//!
//! # Why a read-only database rather than a full disk
//!
//! Filling a real disk is not something a test suite should do, and the failure
//! being modelled is not specifically "no space" — it is "the store refuses to
//! be written". Revoking write permission produces exactly that, at the same
//! layer, without needing a spare filesystem.
//!
//! The `-shm` file has to be revoked too. SQLite writes to the shared-memory
//! index on any WAL connection, so leaving it writable leaves a path that still
//! works, and the test would pass without the failure ever happening.

use std::path::{Path, PathBuf};
use std::process::Command;

fn brain() -> &'static str {
    env!("CARGO_BIN_EXE_brain")
}

struct Store(PathBuf);

impl Store {
    fn new(name: &str) -> Store {
        let d = std::env::temp_dir().join(format!("brain-spool-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let s = Store(d);
        s.ok(&["init"]);
        s
    }

    fn ok(&self, args: &[&str]) -> String {
        let (code, said) = self.raw(args);
        assert!(code == 0, "brain {args:?} exited {code}: {said}");
        said
    }

    fn raw(&self, args: &[&str]) -> (i32, String) {
        let out = Command::new(brain())
            .args(args)
            .env("BRAIN_HOME", &self.0)
            .output()
            .expect("brain runs");
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap_or(-1), text)
    }

    /// Make the log refuse writes, the way a failing disk would.
    fn seal(&self) {
        for f in ["events.db", "events.db-shm", "events.db-wal"] {
            let p = self.0.join(f);
            if p.exists() {
                set_mode(&p, 0o444);
            }
        }
    }

    fn unseal(&self) {
        for f in ["events.db", "events.db-shm", "events.db-wal"] {
            let p = self.0.join(f);
            if p.exists() {
                set_mode(&p, 0o644);
            }
        }
    }

    fn notes(&self) -> Vec<String> {
        self.ok(&["timeline", "1000"])
            .lines()
            .filter(|l| l.starts_with("  "))
            .map(|l| l.trim().to_string())
            .collect()
    }
}

fn set_mode(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

impl Drop for Store {
    fn drop(&mut self) {
        self.unseal();
        std::fs::remove_dir_all(&self.0).ok();
    }
}

#[test]
#[ignore = "manipulates file permissions; run with --ignored"]
fn a_capture_survives_a_store_that_will_not_open() {
    // The case the spool exists for, and the one an inner fallback misses: the
    // failure is in *opening* the store, so a capture that is only built after
    // a successful open has nothing to rescue.
    let s = Store::new("unopenable");
    s.seal();

    let said = s.ok(&["+", "written while the disk was gone"]);
    assert!(said.contains("spooled"), "{said}");
    assert!(
        said.contains("safe"),
        "the message must say the capture is kept: {said}"
    );
}

#[test]
#[ignore = "manipulates file permissions; run with --ignored"]
fn a_spooled_capture_reports_success() {
    // Exit zero. The capture was accepted, which is what the exit code is
    // reporting — telling the user their note failed, when it did not, would
    // send them off to retype something already safe.
    let s = Store::new("exit-code");
    s.seal();
    let (code, _) = s.raw(&["+", "still accepted"]);
    assert_eq!(code, 0);
}

#[test]
#[ignore = "manipulates file permissions; run with --ignored"]
fn a_spooled_capture_lands_when_the_store_recovers() {
    let s = Store::new("recovers");
    s.ok(&["+", "written while everything worked"]);

    s.seal();
    s.ok(&["+", "written while the disk was gone"]);
    s.unseal();

    // The next successful write is the moment the store is known to be usable
    // again. A timer could not have known.
    s.ok(&["+", "written after it came back"]);

    let notes = s.notes();
    assert_eq!(notes.len(), 3, "every capture is present: {notes:?}");
    assert!(
        notes.iter().any(|n| n.contains("while the disk was gone")),
        "the spooled one is in the log: {notes:?}"
    );
}

#[test]
#[ignore = "manipulates file permissions; run with --ignored"]
fn opening_the_store_for_writing_drains_the_spool() {
    // The other moment the store is known to be writable: a process starting
    // and taking the writer lock. Here that is any command that writes; in a
    // running system it is the resident overlay starting up.
    let s = Store::new("startup");
    s.seal();
    s.ok(&["+", "written while the disk was gone"]);
    s.unseal();

    // A write command whose own work has nothing to do with the spool.
    s.ok(&["set", "timeline.limit", "25"]);

    let timeline = s.ok(&["timeline"]);
    assert!(
        timeline.contains("while the disk was gone"),
        "taking the writer lock should have drained the spool: {timeline}"
    );
}

#[test]
#[ignore = "manipulates file permissions; run with --ignored"]
fn a_read_command_does_not_drain_the_spool() {
    // Reading opens the store read-only and takes no writer lock, which is
    // what makes diagnostics work while something else is writing. It follows
    // that a read cannot commit anything — and pretending otherwise would mean
    // `timeline` quietly needing write access to the archive.
    let s = Store::new("read-only");
    s.seal();
    s.ok(&["+", "written while the disk was gone"]);
    s.unseal();

    let timeline = s.ok(&["timeline"]);
    assert!(
        !timeline.contains("while the disk was gone"),
        "a read must not have written anything: {timeline}"
    );
    // And it is still safe, still reported, still waiting for a writer.
    assert!(s.ok(&["status"]).contains("spooled"));
}

#[test]
#[ignore = "manipulates file permissions; run with --ignored"]
fn nothing_is_committed_twice() {
    // Draining commits and then clears. Killed in between — or simply run
    // again — the same records are re-read, and the log must recognise them.
    let s = Store::new("once");
    s.seal();
    s.ok(&["+", "written while the disk was gone"]);
    s.unseal();

    s.ok(&["timeline"]);
    s.ok(&["timeline"]);
    s.ok(&["+", "something afterwards"]);

    let notes = s.notes();
    let repeats = notes
        .iter()
        .filter(|n| n.contains("while the disk was gone"))
        .count();
    assert_eq!(repeats, 1, "committed {repeats} times: {notes:?}");
}

#[test]
#[ignore = "manipulates file permissions; run with --ignored"]
fn the_spool_is_reported_while_it_holds_anything() {
    // Immediately, and as something needing attention. The condition gets
    // worse with every further capture rather than with the calendar, so it
    // cannot wait for a daily summary.
    let s = Store::new("reported");
    s.seal();
    s.ok(&["+", "written while the disk was gone"]);

    let status = s.ok(&["status"]);
    assert!(status.contains("Needs attention"), "{status}");
    assert!(status.contains("spooled"), "{status}");

    s.unseal();
    s.ok(&["set", "timeline.limit", "25"]);
    let status = s.ok(&["status"]);
    assert!(
        status.contains("Nothing needs your attention"),
        "the condition should clear once the spool is empty: {status}"
    );
}

#[test]
#[ignore = "manipulates file permissions; run with --ignored"]
fn a_restored_capture_keeps_the_time_it_was_written() {
    // The sequence number is assigned at drain, but the moment and the zone
    // belong to the capture. Taking them from the drain would file a note
    // under the day the disk recovered rather than the day it was written.
    let s = Store::new("timing");
    s.seal();
    s.ok(&["+", "written while the disk was gone"]);
    s.unseal();
    s.ok(&["set", "timeline.limit", "25"]);

    let json = s.ok(&["timeline", "--json"]);
    assert!(
        json.contains("\"tz\":\""),
        "the capture's zone must survive the spool: {json}"
    );
}
