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

fn capture() -> &'static str {
    env!("CARGO_BIN_EXE_capture")
}

struct Store(PathBuf);

impl Store {
    fn empty(name: &str) -> Store {
        let d = std::env::temp_dir().join(format!("capture-rt-{name}-{}", std::process::id()));
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
        assert!(out.0 == 0, "capture {args:?} exited {}: {}", out.0, out.1);
        out.1
    }

    fn raw(&self, args: &[&str]) -> (i32, String) {
        let out = Command::new(capture())
            .args(args)
            .env("CAPTURE_HOME", &self.0)
            .output()
            .expect("capture runs");
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
    // The credential reason specifically, and the remedy that goes with it.
    //
    // "without their text" was the whole assertion, and *both* of the export's
    // withholding messages contain that phrase — so an export that reported the
    // redaction reason instead would have passed, and the advice would have
    // been wrong for the notes it was printed beside.
    assert!(said.contains("look like they hold credentials"), "{said}");
    assert!(
        said.contains("capture unflag"),
        "the remedy is missing: {said}"
    );
    assert!(
        !said.contains("because a redaction names them"),
        "a credential flag was reported as a redaction: {said}"
    );

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
    assert!(said.contains("look like they hold credentials"), "{said}");

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

/// A restore brings back what the archive *meant*, not only what it held.
///
/// Everything below `notes` is derived: a list's items are expanded from the
/// capture's text, a promotion is read from it, an alert's moment comes from
/// the promotion. None of it is in the export, because none of it is in the
/// log — the export carries events and the projection is rebuilt from them.
///
/// So this asserts the rebuild, not the copy. If it ever fails, the restore
/// will look successful — the right number of notes, the right text — and the
/// lists will be gone, which is the shape of failure somebody discovers in a
/// shop rather than at a terminal.
///
/// Written after an uninstall took a phone's archive with it. The machinery
/// was all there and tested; what nobody had checked was whether what came
/// back was still usable rather than merely present.
#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn a_restore_brings_back_the_structure_too() {
    let a = Store::fresh("structured");
    a.ok(&["+", "#groceries: milk, eggs, bread"]);
    a.ok(&["+", "dentist thursday 2pm"]);
    a.ok(&["+", "call the dentist tomorrow"]);
    a.ok(&["tick", "groceries", "milk"]);
    a.ok(&["export"]);

    let b = Store::rolled_back_from(&a, "structured-back");
    b.ok(&["restore", a.export_dir().to_str().unwrap()]);

    // The list is a list again, and the tick came with it — the tick is an
    // event, so it is in the export; the *items* are not, and have to be
    // re-expanded from the capture's text.
    let list = b.ok(&["list", "groceries"]);
    assert!(list.contains("milk"), "the items were re-expanded: {list}");
    assert!(list.contains("eggs"));
    assert!(list.contains("bread"));
    assert!(
        list.contains("[x] milk"),
        "and what was ticked is still ticked: {list}",
    );

    // The promotions were re-read, so the things that speak still speak.
    let due = b.ok(&["due"]);
    assert!(
        due.contains("dentist thursday 2pm"),
        "an event still has its moment after a restore: {due}",
    );
    assert!(
        due.contains("call the dentist tomorrow"),
        "and so does a task with a due date: {due}",
    );
}

// ------------------------------------------- redactions reach the export ----
//
// §1.1 limit 3: "The exporter therefore applies retained redactions when
// writing *new* export files". It did not — the strip set was credential flags
// and nothing else, and redacted content stayed out of exports only
// incidentally, because by the time destruction had run the body was already
// gone.
//
// The gap is the interval between ordering a destruction and carrying it out:
// an export firing in between wrote the full plaintext of a note already
// ordered destroyed into a brand-new file, one created *after* the order, which
// redaction can never reach afterwards.

/// Whether a phrase appears anywhere in the export directory's files.
fn export_contains(s: &Store, needle: &str) -> bool {
    let dir = s.export_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return false;
    };
    for e in entries.flatten() {
        if let Ok(bytes) = std::fs::read(e.path())
            && String::from_utf8_lossy(&bytes).contains(needle)
        {
            return true;
        }
    }
    false
}

#[test]
fn an_export_does_not_carry_a_note_already_ordered_destroyed() {
    let s = Store::fresh("export-redacted");
    let slug = s.ok(&["+", "the passphrase is correct horse"]);
    let slug = slug.trim();
    s.ok(&["settle"]);

    // Ordered, not yet carried out: the window has not elapsed, so the body is
    // still in the log and this is exactly the interval that leaked.
    s.ok(&["redact", slug, "--force"]);
    s.ok(&["export"]);

    assert!(
        !export_contains(&s, "correct horse"),
        "an export written after the redaction carried the text anyway"
    );
}

#[test]
fn an_export_still_carries_an_ordinary_note() {
    // The guard against stripping everything, which would also satisfy the
    // assertion above.
    let s = Store::fresh("export-ordinary");
    s.ok(&["+", "an unremarkable note about tulips"]);
    s.ok(&["settle"]);
    s.ok(&["export"]);

    assert!(
        export_contains(&s, "tulips"),
        "an ordinary note was withheld from its own backup"
    );
}

#[test]
fn a_cancelled_redaction_exports_whole() {
    // Cancelling means the note stands, so the export must carry it.
    let s = Store::fresh("export-cancelled");
    let slug = s.ok(&["+", "a note about marigolds"]);
    let slug = slug.trim();
    s.ok(&["settle"]);

    let out = s.ok(&["redact", slug, "--force"]);
    // The id the redaction printed, as `--cancel` takes it.
    let id = out
        .split_whitespace()
        .last()
        .expect("redact prints an id to cancel with")
        .to_string();
    s.ok(&["redact", "--cancel", &id]);
    s.ok(&["export"]);

    assert!(
        export_contains(&s, "marigolds"),
        "a cancelled redaction still withheld the note"
    );
}

#[test]
fn an_export_reports_a_redaction_differently_from_a_credential() {
    // Two reasons a note travels without its text, counted apart because what
    // a person should do about each is opposite: a credential flag over-flags
    // on purpose and may be a false positive to clear, and a redaction is a
    // decision nothing should undo.
    //
    // Both messages contain "without their text", which is all either test
    // asserted — so one could have been printed in place of the other and
    // nothing would have noticed, and "If one of those is an ordinary note:
    // capture unflag <name>" would have appeared beside a note somebody had
    // asked to destroy.
    // A name of its own: `export-redacted` is taken, and two tests sharing a
    // scratch directory lock each other out of it.
    let a = Store::fresh("export-reason");
    let slug = a.ok(&[
        "+", // Deliberately bland. The first attempt read "an ordinary note with
        // nothing secret in it" and was flagged as a credential — the predicate
        // over-flags on purpose, and the word "secret" is enough.
        "lunch with sarah on tuesday",
    ]);
    let slug = slug.trim().to_string();
    a.ok(&["settle"]);
    a.ok(&["redact", "--force", &slug]);

    let said = a.ok(&["export"]);

    assert!(
        said.contains("because a redaction names them"),
        "the redaction reason was not reported: {said}"
    );
    // And no remedy, because there is nothing to clear.
    assert!(
        !said.contains("capture unflag"),
        "an export offered to unflag a note somebody asked to destroy: {said}"
    );
    assert!(
        !said.contains("look like they hold credentials"),
        "a redaction was reported as a credential flag: {said}"
    );
}

/// Two machines edit one note, and somebody dismisses the conflict.
///
/// `resolve` was the last write verb with no test, and it needed a real
/// divergence to have anything to do — which only a second machine can
/// produce. Both stores edit the same version, so neither edit names the other
/// as its predecessor and the fold has two tips and no way to order them. That
/// is the case §6 calls the failure worth avoiding, and the one `resolve` is
/// the answer to.
#[test]
fn a_conflict_between_two_machines_is_dismissed_and_nothing_is_lost() {
    let a = Store::fresh("resolve-a");
    let b = Store::fresh("resolve-b");

    let slug = a.ok(&["+", "the migration plan"]).trim().to_string();
    a.ok(&["export"]);
    b.ok(&["restore", a.export_dir().to_str().unwrap()]);
    assert!(b.ok(&["show", &slug]).contains("the migration plan"));

    // One note, two machines, neither having seen the other's edit. This is
    // ordinary rather than exotic: it is what a phone edited on a train and a
    // laptop edited at a desk produce.
    a.ok(&["edit", &slug, "the migration plan, revised on a"]);
    b.ok(&["edit", &slug, "the migration plan, revised on b"]);

    a.ok(&["export"]);
    b.ok(&["restore", a.export_dir().to_str().unwrap()]);

    // Reported, not prompted. A divergence is information about the note, so
    // the page says how many versions there are and does not ask anybody to
    // clear it before reading on.
    let diverged = b.ok(&["show", &slug]);
    assert!(
        diverged.contains("versions  2 independent"),
        "two independent edits were not reported as a divergence: {diverged}"
    );

    // What the local machine wrote is what it still shows. Picking the incoming
    // version would lose an edit silently, which is the whole failure.
    assert!(
        diverged.contains("revised on b"),
        "the local edit was displaced by the incoming one: {diverged}"
    );

    let before = events_in(&b);
    let said = b.ok(&["resolve", &slug]);
    assert!(said.contains("dismissed 1 competing version"), "{said}");

    let after = b.ok(&["show", &slug]);
    assert!(
        !after.contains("versions "),
        "the divergence was still reported after being dismissed: {after}"
    );

    // "both remain in the log" is the command's own claim, and this is what
    // makes it checkable: dismissing appends an event and removes none, so the
    // log only grew and still verifies. Nothing in this system destroys
    // anything except redaction.
    assert!(
        events_in(&b) > before,
        "dismissing a divergence wrote nothing"
    );
    assert!(
        b.ok(&["verify"]).contains("checks out"),
        "the log does not verify after a resolve"
    );

    // Once. A second dismissal has nothing to dismiss, and saying so beats
    // appending an event that means nothing.
    let (code, said) = b.raw(&["resolve", &slug]);
    assert_ne!(code, 0, "resolved a note with no divergence: {said}");
    assert!(said.contains("no divergence"), "{said}");

    // The machine that was not asked still shows its own edit, and does not
    // report a divergence it never saw. Whether the dismissal itself travels is
    // *not* established here: restoring b's export into a is refused, because
    // a holds no key for b's origin and §5.3 admits a device rather than
    // trusting an export. That wants the sync tests and a roster, not this one.
    let untouched = a.ok(&["show", &slug]);
    assert!(
        untouched.contains("revised on a"),
        "the machine that was not asked lost its own edit: {untouched}"
    );
    assert!(
        !untouched.contains("versions "),
        "a reported a divergence it was never sent: {untouched}"
    );
}

/// How many events a store holds, from what `verify` says it checked.
///
/// Asked through the binary rather than by opening the database, so the test
/// stays a test of the command line.
fn events_in(s: &Store) -> usize {
    let said = s.ok(&["verify"]);
    let at = said.find("checked ").expect("verify says what it checked") + "checked ".len();
    said[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .expect("a count")
}
