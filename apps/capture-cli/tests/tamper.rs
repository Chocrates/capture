//! Damaging a store on purpose, and checking that `capture verify` notices.
//!
//! # Why this is not a unit test
//!
//! A unit test would hand `verify` a constructed event and check the verdict.
//! That tests the comparison and nothing else. What matters here is whether
//! damage to the **stored bytes** — the thing a failing disk, a truncated
//! restore, or someone with a SQLite shell actually produces — is caught by the
//! command a person runs.
//!
//! So each case opens the real database, changes it the way corruption would,
//! and runs the real binary.
//!
//! # A check that cannot fail is decoration
//!
//! `verify` reporting a clean store proves nothing on its own; a command that
//! printed "everything checks out" unconditionally would pass that test too.
//! Every case below therefore asserts both that the damage is caught *and* that
//! it is named specifically, because "something is wrong" is not enough to act
//! on.

use std::path::{Path, PathBuf};
use std::process::Command;

fn capture() -> &'static str {
    env!("CARGO_BIN_EXE_capture")
}

/// A store with a few events in it, ready to be broken.
struct Store(PathBuf);

impl Store {
    fn new(name: &str) -> Store {
        let d = std::env::temp_dir().join(format!("capture-tamper-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let s = Store(d);

        s.run(&["init"]);
        let slug = s.run(&["+", "budget planning for next quarter"]);
        let slug = slug.trim();
        s.run(&["tag", slug, "finance"]);
        s.run(&["edit", slug, "revised numbers for the quarter"]);
        s
    }

    fn run(&self, args: &[&str]) -> String {
        let out = Command::new(capture())
            .args(args)
            .env("CAPTURE_HOME", &self.0)
            .output()
            .expect("capture runs");
        assert!(
            out.status.success(),
            "capture {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// Change the stored bytes the way corruption would.
    fn corrupt(&self, sql: &str) {
        let ok = Command::new("sqlite3")
            .arg(self.0.join("events.db"))
            .arg(sql)
            .status()
            .expect("sqlite3 is available");
        assert!(ok.success(), "corrupting the store failed");
    }

    /// Run `verify`, returning its exit code and what it said.
    fn verify(&self) -> (i32, String) {
        let out = Command::new(capture())
            .arg("verify")
            .env("CAPTURE_HOME", &self.0)
            .output()
            .expect("capture verify runs");
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap_or(-1), text)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// The exit code reserved for a damaged archive.
const INTEGRITY: i32 = 3;

#[test]
#[ignore = "needs the sqlite3 shell; run with --ignored"]
fn an_untouched_store_verifies() {
    // The control. Without it, every case below could be passing because
    // `verify` always reports damage.
    let s = Store::new("clean");
    let (code, said) = s.verify();
    assert_eq!(code, 0, "a clean store must verify: {said}");
    assert!(said.contains("checks out"), "{said}");
}

#[test]
#[ignore = "needs the sqlite3 shell; run with --ignored"]
fn a_tampered_body_is_caught() {
    // The commitment in the header no longer describes the payload. This is
    // what an edited note looks like if someone edits it outside the system.
    let s = Store::new("body");
    s.corrupt("UPDATE events SET body = X'DEADBEEF' WHERE kind = 1;");
    let (code, said) = s.verify();
    assert_eq!(code, INTEGRITY, "{said}");
    assert!(said.contains("body does not match"), "{said}");
}

#[test]
#[ignore = "needs the sqlite3 shell; run with --ignored"]
fn an_altered_header_is_caught() {
    // The id is computed over the header, so changing any part of it makes the
    // row's key stop describing its contents.
    let s = Store::new("header");
    s.corrupt("UPDATE events SET kind = 99 WHERE seq = 3;");
    let (code, said) = s.verify();
    assert_eq!(code, INTEGRITY, "{said}");
    assert!(said.contains("its header produces"), "{said}");
}

#[test]
#[ignore = "needs the sqlite3 shell; run with --ignored"]
fn a_forged_signature_is_caught() {
    let s = Store::new("sig");
    s.corrupt(&format!(
        "UPDATE events SET sig = X'{}' WHERE seq = 2;",
        "00".repeat(64)
    ));
    let (code, said) = s.verify();
    assert_eq!(code, INTEGRITY, "{said}");
    assert!(said.contains("signature does not verify"), "{said}");
}

#[test]
#[ignore = "needs the sqlite3 shell; run with --ignored"]
fn a_missing_event_is_caught() {
    // The case a chain walk exists for. Nothing about the remaining events is
    // individually wrong — each one verifies perfectly — and the archive is
    // still damaged, because a sequence number that nothing fills can never be
    // filled again.
    let s = Store::new("gap");
    s.corrupt("DELETE FROM events WHERE seq = 3;");
    let (code, said) = s.verify();
    assert_eq!(code, INTEGRITY, "{said}");
    assert!(said.contains("an event is missing"), "{said}");
}

#[test]
#[ignore = "needs the sqlite3 shell; run with --ignored"]
fn damage_is_reported_once_per_event() {
    // A body that no longer matches its commitment fails the signature check
    // and a body-hash check both, and reporting it twice would inflate the
    // count a reader uses to judge how bad things are.
    let s = Store::new("once");
    s.corrupt("UPDATE events SET body = X'DEADBEEF' WHERE kind = 1;");
    let (_, said) = s.verify();
    let mentions = said.matches("body does not match").count();
    assert_eq!(mentions, 1, "reported {mentions} times:\n{said}");
}

#[test]
#[ignore = "needs the sqlite3 shell; run with --ignored"]
fn a_damaged_archive_does_not_report_as_a_usage_error() {
    // Three is its own code because a script has to tell "you typed it wrong"
    // from "the archive is broken". The first is retryable; the second is not.
    let s = Store::new("codes");
    s.corrupt("DELETE FROM events WHERE seq = 3;");
    let (code, _) = s.verify();
    assert_eq!(code, INTEGRITY);
    assert_ne!(code, 1, "not a usage error");
    assert_ne!(code, 2, "not a missing note");
}

#[test]
#[ignore = "needs the sqlite3 shell; run with --ignored"]
fn verifying_does_not_need_the_writer() {
    // A check that only runs while the system is healthy is unavailable at the
    // one moment it is wanted.
    let s = Store::new("locked");
    let lock = capture_store::lock::Writer::acquire(&s.path().join("writer.lock"), "test")
        .unwrap()
        .expect("the lock is free");
    let (code, said) = s.verify();
    drop(lock);
    assert_eq!(
        code, 0,
        "verify must run while another process writes: {said}"
    );
}

/// A projection an older build wrote is refused by name, and a write fixes it.
///
/// Found on the real archive after an install: `capture status` answered
/// "storage: no such table: late_cancels", because the projection on disk was
/// built by a build that predated the table. Not damage — it is derived — but
/// a read cannot rebuild it, so the message has to say what will.
#[test]
#[ignore = "needs the sqlite3 shell; run with --ignored"]
fn a_projection_from_an_older_build_says_how_to_fix_it() {
    let s = Store::new("stale");
    // The newest generation, as an older build would have left it.
    let state = std::fs::read_dir(s.path())
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("state.") && n.ends_with(".db"))
        })
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .expect("a projection exists");
    let ok = Command::new("sqlite3")
        .arg(&state)
        .arg("UPDATE meta SET v = '1' WHERE k = 'schema';")
        .status()
        .expect("sqlite3 is available");
    assert!(ok.success());

    let out = Command::new(capture())
        .args(["timeline"])
        .env("CAPTURE_HOME", s.path())
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success(),
        "a stale projection was read as current"
    );
    assert!(said.contains("older version"), "{said}");
    assert!(
        said.contains("capture settle"),
        "it should name the fix: {said}"
    );
    assert!(!said.contains("no such table"), "{said}");

    // And the fix it names works.
    s.run(&["settle"]);
    assert!(s.run(&["timeline"]).contains("revised numbers"));
}
