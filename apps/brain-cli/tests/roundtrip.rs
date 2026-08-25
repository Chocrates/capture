//! Export a store, restore it somewhere else, and compare.
//!
//! # The claim being checked
//!
//! "Restore reproduces what the original produced." That is a statement about
//! two whole stores, not about any function, and it is the only claim that
//! matters on the day it is needed. A unit test on the encoder would pass while
//! a restore produced a store missing half its notes.
//!
//! # A restore is a rolled-back device, not a new one
//!
//! Roster membership is keyed by public key, and the key lives in the store's
//! config directory rather than in the export — the export holds events. So the
//! case modelled here is the real one: a device whose databases are gone and
//! whose key survived, which is what a restore from backup is.
//!
//! Restoring onto hardware that has never been a member is a different problem
//! — that device has no key on the roster, so it could read the archive but
//! never author into it. Solving that is device pairing, and it does not exist
//! yet; pretending otherwise here would be testing a story rather than a path.

use std::path::{Path, PathBuf};
use std::process::Command;

fn brain() -> &'static str {
    env!("CARGO_BIN_EXE_brain")
}

struct Store(PathBuf);

impl Store {
    fn empty(name: &str) -> Store {
        let d = std::env::temp_dir().join(format!("brain-rt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Store(d)
    }

    fn fresh(name: &str) -> Store {
        let s = Store::empty(name);
        s.ok(&["init"]);
        s
    }

    /// A store holding the same device identity, with nothing else.
    ///
    /// What a machine looks like after its databases are lost.
    fn rolled_back_from(other: &Store, name: &str) -> Store {
        let s = Store::empty(name);
        copy_dir(&other.0.join("config"), &s.0.join("config"));
        s
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.raw(args);
        assert!(out.0 == 0, "brain {args:?} exited {}: {}", out.0, out.1);
        out.1
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

    fn export_dir(&self) -> PathBuf {
        self.0.join("export")
    }

    /// Every note, as JSON, for comparing two stores.
    fn notes(&self) -> Vec<String> {
        let raw = self.ok(&["timeline", "10000", "--json"]);
        // Compared as text rather than parsed: the point is that the two
        // stores agree on every field, and parsing would let a field silently
        // stop being compared the day it is added.
        raw.trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split("},{")
            .map(|s| s.trim_matches(|c| c == '{' || c == '}').to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn a_restored_store_holds_what_the_original_held() {
    let a = Store::fresh("origin");
    let slug = a.ok(&["+", "budget planning for next quarter"]);
    let slug = slug.trim();
    a.ok(&["tag", slug, "finance"]);
    a.ok(&["alias", slug, "q4-budget"]);
    a.ok(&["edit", slug, "revised numbers for the quarter"]);
    a.ok(&["+", "a second note mentioning [[q4-budget]]"]);
    a.ok(&["export"]);

    let b = Store::rolled_back_from(&a, "restored");
    b.ok(&["restore", a.export_dir().to_str().unwrap()]);

    assert_eq!(
        a.notes(),
        b.notes(),
        "the restored store must render what the original rendered"
    );
    // And the links came with it, not just the text.
    assert!(b.ok(&["links", "q4-budget"]).contains("linked from"));
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn a_restored_store_verifies() {
    let a = Store::fresh("verify-origin");
    a.ok(&["+", "something worth keeping around"]);
    a.ok(&["export"]);

    let b = Store::rolled_back_from(&a, "verify-restored");
    b.ok(&["restore", a.export_dir().to_str().unwrap()]);

    let (code, said) = b.raw(&["verify"]);
    assert_eq!(code, 0, "a restored store must verify cleanly: {said}");
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn export_is_incremental() {
    let a = Store::fresh("incremental");
    a.ok(&["+", "the first thing written down"]);
    a.ok(&["export"]);

    // Nothing new: the second run must not re-emit the archive.
    assert!(a.ok(&["export"]).contains("Nothing new"));

    a.ok(&["+", "the second thing written down"]);
    let said = a.ok(&["export"]);
    assert!(said.contains("exported 1 event"), "{said}");
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn a_credential_shaped_note_travels_without_its_text() {
    // The whole reason for the flag: an export directory is copied somewhere
    // redaction can never reach.
    let a = Store::fresh("credential");
    a.ok(&["+", "my aws secret key is AKIAIOSFODNN7EXAMPLE"]);
    let said = a.ok(&["export"]);
    assert!(said.contains("without their text"), "{said}");

    // Not one byte of it in the file.
    let mut found = false;
    for f in std::fs::read_dir(a.export_dir()).unwrap() {
        let bytes = std::fs::read(f.unwrap().path()).unwrap();
        if String::from_utf8_lossy(&bytes).contains("AKIAIOSFODNN7EXAMPLE") {
            found = true;
        }
    }
    assert!(!found, "the credential reached the export file");
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn a_stripped_note_still_exists_after_a_restore() {
    // Losing the text is the intended cost. Losing the note would be data loss,
    // and it would happen quietly — the timeline would simply be shorter.
    let a = Store::fresh("stripped");
    a.ok(&["+", "my aws secret key is AKIAIOSFODNN7EXAMPLE"]);
    a.ok(&["+", "an ordinary note alongside it"]);
    a.ok(&["export"]);

    let b = Store::rolled_back_from(&a, "stripped-restored");
    b.ok(&["restore", a.export_dir().to_str().unwrap()]);

    let timeline = b.ok(&["timeline"]);
    assert_eq!(b.notes().len(), 2, "both notes must come back:\n{timeline}");
    assert!(timeline.contains("no readable text"), "{timeline}");
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn clearing_the_flag_puts_a_note_back_in_the_export() {
    // The rail on a predicate that deliberately over-flags. Without this, an
    // ordinary note that happens to match is missing from every backup forever.
    let a = Store::fresh("unflag");
    let slug = a.ok(&["+", "remind me to rotate the api key on friday"]);
    let slug = slug.trim();
    let said = a.ok(&["export"]);
    assert!(said.contains("without their text"), "{said}");

    a.ok(&["unflag", slug]);
    a.ok(&["+", "forcing a fresh export file"]);
    let said = a.ok(&["export"]);
    assert!(
        !said.contains("without their text"),
        "the note should be exported in full now: {said}"
    );
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn a_capture_after_a_restore_uses_a_new_origin() {
    // The sequence counter has just moved backwards. Authoring under the old
    // origin would reuse positions peers already hold, losing events in both
    // directions with nothing to notice it.
    let a = Store::fresh("origin-a");
    a.ok(&["+", "written before the restore happened"]);
    a.ok(&["export"]);

    let b = Store::rolled_back_from(&a, "origin-b");
    let said = b.ok(&["restore", a.export_dir().to_str().unwrap()]);
    assert!(said.contains("new origin"), "{said}");

    let slug = b.ok(&["+", "written after the restore happened"]);
    let log = b.ok(&["log", slug.trim()]);
    assert!(log.contains("seq 2"), "the new origin starts fresh: {log}");

    let (code, said) = b.raw(&["verify"]);
    assert_eq!(code, 0, "{said}");
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn restoring_from_somewhere_with_no_exports_says_so() {
    let a = Store::fresh("empty-export");
    let (code, said) = a.raw(&["restore", "/nonexistent/place"]);
    assert_eq!(code, 2, "a missing export directory is 'not found': {said}");
}
