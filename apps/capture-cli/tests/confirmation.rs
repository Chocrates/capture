//! The prompt in front of the only irreversible operation.
//!
//! # Prose is asserted here, deliberately
//!
//! `scenarios.rs` says in its header that human-readable output must not be
//! asserted, because wording is meant to change. That rule is right and this
//! file breaks it, which is why it is a separate file.
//!
//! Here the prose *is* the contract. §1.1 makes the confirmation's content a
//! requirement — it must name what actually goes — rather than a presentation
//! choice. "The prompt says the recording goes too" is a claim about behaviour
//! that happens to be delivered as text, and the alternative to asserting it is
//! what was there before: nothing.
//!
//! # Why none of this existed
//!
//! Every `redact` in the suite passes `--force`, which exists precisely to skip
//! this prompt, and nothing in the workspace piped stdin to the binary at all.
//! So the guard in front of the one operation that cannot be undone was the
//! least exercised code in the repo, for want of a harness method nothing else
//! needed.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn capture() -> &'static str {
    env!("CARGO_BIN_EXE_capture")
}

struct Store(PathBuf);

impl Store {
    fn new(name: &str) -> Store {
        let d = std::env::temp_dir().join(format!("capture-conf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let s = Store(d);
        s.ok(&["init"]);
        s
    }

    fn ok(&self, args: &[&str]) -> String {
        let (code, out) = self.run(args);
        assert!(code == 0, "capture {args:?} exited {code}: {out}");
        out
    }

    fn run(&self, args: &[&str]) -> (i32, String) {
        let out = Command::new(capture())
            .args(args)
            .env("CAPTURE_HOME", &self.0)
            .output()
            .expect("capture runs");
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap_or(-1), text)
    }

    /// Run a command and answer whatever it asks.
    ///
    /// The method the suite never had. `.output()` gives the child no stdin, so
    /// a command that reads a line gets nothing — which is why every existing
    /// `redact` passes `--force` and why this prompt went unchecked.
    fn answering(&self, args: &[&str], typed: &str) -> (i32, String) {
        let mut child = Command::new(capture())
            .args(args)
            .env("CAPTURE_HOME", &self.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("capture runs");
        child
            .stdin
            .take()
            .expect("stdin was piped")
            .write_all(typed.as_bytes())
            .expect("the prompt took the answer");
        let out = child.wait_with_output().expect("capture finishes");
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap_or(-1), text)
    }

    /// Run a command as of a given moment.
    ///
    /// The store's clock, not only the reading one. Until `Session::open_at`
    /// existed this could not be done at all from here: the work a deadline
    /// triggers runs *during* the open, because §8.7 makes process startup the
    /// floor rather than trusting a timer unit to be running, and so nothing
    /// reachable after the session exists can move time.
    fn at(&self, when_ms: i64, args: &[&str]) -> String {
        let out = Command::new(capture())
            .args(args)
            .env("CAPTURE_HOME", &self.0)
            .env("CAPTURE_NOW_MS", when_ms.to_string())
            .output()
            .expect("capture runs");
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success(), "capture {args:?}: {text}");
        text
    }

    fn capture_note(&self, text: &str) -> String {
        self.ok(&["+", text]).trim().to_string()
    }

    /// Whether the note still has its words.
    ///
    /// The assertion that matters. "Destroys nothing" is a claim about the
    /// store, not about what was printed — and the failure worth catching is a
    /// prompt that says "Nothing destroyed" and queues the redaction anyway,
    /// which a test reading only stdout would pass.
    fn still_readable(&self, slug: &str, words: &str) -> bool {
        self.ok(&["show", slug]).contains(words)
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_near_miss_destroys_nothing() {
    let s = Store::new("near-miss");
    let slug = s.capture_note("the thing I might regret destroying");
    s.ok(&["settle"]);

    let (code, out) = s.answering(&["redact", &slug], &format!("{slug}x\n"));

    assert_eq!(code, 0, "declining is not a failure: {out}");
    assert!(
        s.still_readable(&slug, "the thing I might regret destroying"),
        "a near miss destroyed the note anyway"
    );
}

#[test]
fn an_empty_answer_destroys_nothing() {
    let s = Store::new("empty");
    let slug = s.capture_note("the thing I might regret destroying");
    s.ok(&["settle"]);

    let (code, out) = s.answering(&["redact", &slug], "\n");

    assert_eq!(code, 0, "declining is not a failure: {out}");
    assert!(
        s.still_readable(&slug, "the thing I might regret destroying"),
        "a blank line destroyed the note"
    );
}

#[test]
fn the_name_with_something_after_it_destroys_nothing() {
    // Different from the trailing newline a terminal supplies, which is
    // forgiven on purpose — `typed.trim()` exists because some shells add more
    // than one. "name yes" is not the name, and the difference between
    // tolerating how a line is delivered and tolerating what somebody typed is
    // the whole of what this prompt is for.
    let s = Store::new("trailing");
    let slug = s.capture_note("the thing I might regret destroying");
    s.ok(&["settle"]);

    let (code, _) = s.answering(&["redact", &slug], &format!("{slug} yes\n"));

    assert_eq!(code, 0);
    assert!(
        s.still_readable(&slug, "the thing I might regret destroying"),
        "the name with something after it was accepted as the name"
    );
}

#[test]
fn the_exact_name_orders_the_destruction() {
    // The guard against the three above passing for the wrong reason: a prompt
    // that refused everything would satisfy them all.
    let s = Store::new("exact");
    let slug = s.capture_note("the thing I meant to destroy");
    s.ok(&["settle"]);

    let (code, out) = s.answering(&["redact", &slug], &format!("{slug}\n"));

    assert_eq!(code, 0, "{out}");
    assert!(
        !s.still_readable(&slug, "the thing I meant to destroy"),
        "the exact name was typed and the content is still readable: {out}"
    );
}

#[test]
fn the_prompt_mentions_no_recording_when_there_is_none() {
    // Only the negative half, and the name says so.
    //
    // The positive half — "a note with a recording says the recording goes
    // too" — cannot be reached from here: the command line has no way to
    // attach a recording, because recordings come from the phone's voice
    // capture. Testing it would mean giving `capture-cli` a dev-dependency on
    // `capture-store` so a test could build the capture directly, which would
    // let these tests reach past the binary they exist to drive.
    //
    // So the clause stays unasserted, and that is worth knowing: it has
    // history. `blobs::destroy` was written, tested, and called by nothing, so
    // redaction destroyed the text and left the recording. `blob_redaction.rs`
    // covers the *behaviour* — the bytes go — and this line of the prompt, the
    // user-visible half of that fix, is checked by nothing.
    let s = Store::new("recording");
    let slug = s.capture_note("a note about the meeting");
    s.ok(&["settle"]);

    let without = s.answering(&["redact", &slug], "no\n").1;
    assert!(
        !without.to_lowercase().contains("recording"),
        "a note with no recording was told one would go: {without}"
    );
}

#[test]
fn every_note_that_will_go_is_named() {
    // A flag that widens what the destruction covers has to widen what the
    // prompt says, and by name rather than by count. Somebody who has typed a
    // name once has learned that the prompt means one note.
    let s = Store::new("with-links");
    let target = s.capture_note("the settlement figure");
    s.ok(&["settle"]);
    s.ok(&["alias", &target, "settlement"]);
    let linking = s.capture_note("ask about [[settlement]] before friday");
    s.ok(&["settle"]);

    let out = s.answering(&["redact", "--with-links", &target], "no\n").1;

    assert!(
        out.contains(&linking),
        "the note that would also go was not named: {out}"
    );
    assert!(
        s.still_readable(&target, "the settlement figure"),
        "declining destroyed the target anyway"
    );
    assert!(
        s.still_readable(&linking, "ask about"),
        "declining destroyed the linking note"
    );
}

/// A redaction is carried out, and the copy taken first brings it back.
///
/// §1.2's pre-redact snapshot, and the only test of it through the commands.
/// It needs three things to happen in order — the redaction confirmed, its
/// 30-minute window elapsed, the purge run — and the third is what made this
/// untestable: the purge is drained by the *store's* clock when a session
/// opens, which `CAPTURE_NOW_MS` did not reach until `Session::open_at`.
///
/// Worth the setup because the alternative test, which is all there was,
/// asserts that destruction was *queued*. That is indistinguishable from the
/// content still being on disk, which is the one outcome §1.1 exists to
/// prevent.
#[test]
fn a_destroyed_note_comes_back_from_the_copy_taken_first() {
    let s = Store::new("recover");
    let secret = "the budget is 40k and I should not have written it down";
    let slug = s.capture_note(secret);
    s.ok(&["settle"]);

    let (code, said) = s.answering(
        &["redact", &slug],
        &format!(
            "{slug}
"
        ),
    );
    assert_eq!(code, 0, "{said}");
    assert!(said.contains("will be destroyed in 30 minutes"), "{said}");

    // Still withheld rather than destroyed, and still cancellable. Nothing has
    // happened yet, which is the whole point of the window.
    assert!(
        s.ok(&["show", &slug]).contains("queued for destruction"),
        "the content was destroyed before the window closed"
    );
    assert!(
        s.ok(&["recover"]).contains("Nothing is being kept"),
        "a copy was offered for a redaction that had not run"
    );

    // Past the window, and a write to make the store notice. A read cannot:
    // reads take no writer lock (§8.8) and the drain happens on the writing
    // path, so the thing that moves time has to be a command that writes.
    let later = now_ms() + 40 * 60 * 1000;
    let other = s.capture_note("an unrelated note");
    s.at(later, &["pin", &other]);

    // The slug is gone with the words, because it was made from them. The row
    // is not: a destroyed note is still listed, as `redacted-…`, which is the
    // difference between an archive that lost something and one that says so.
    let timeline = s.ok(&["timeline"]);
    assert!(
        timeline.contains("(content destroyed)"),
        "a destroyed note vanished instead of saying so: {timeline}"
    );
    assert!(
        !timeline.contains("budget"),
        "the destroyed words survived in the listing: {timeline}"
    );

    // And the copy is being kept, with the content, because §1.2 makes a
    // misfire recoverable for thirty days.
    let kept = s.at(later, &["recover"]);
    assert!(
        kept.contains(secret),
        "the copy does not hold the text: {kept}"
    );
    assert!(kept.contains("expires in"), "{kept}");

    let id = kept
        .lines()
        .find(|l| l.contains(secret))
        .and_then(|l| l.split_whitespace().next())
        .expect("the listing names an id")
        .to_string();

    let brought = s.at(later, &["recover", &id]);
    assert!(brought.contains("new"), "{brought}");

    // A new capture, never a resurrection. The redaction still stands, so
    // saying "restored" would claim it had been undone — and somebody would
    // stop looking for the other copies.
    let after = s.ok(&["search", "budget"]);
    assert!(after.contains("40k"), "the text did not come back: {after}");
    assert!(
        s.ok(&["timeline"]).contains("(content destroyed)"),
        "recovering undid the redaction"
    );
}

/// This machine's clock, for moving past a window.
///
/// `jiff` rather than `std::time`, because the purity lint forbids
/// `SystemTime::now` anywhere in the tree — one clock, so that a second one
/// cannot be introduced accidentally and then disagree.
fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

/// What stays behind is named too: the notes whose links will break, and the
/// pages that will be filed under nothing (pages-that-hold-up G10).
#[test]
fn the_prompt_names_what_will_link_to_nothing_and_what_will_be_unfiled() {
    let s = Store::new("page-kin");
    s.ok(&["page", "new", "Trip"]);
    s.ok(&["page", "new", "Trip packing", "--under", "trip"]);
    let linking = s.capture_note("see the [[trip]] page");
    s.ok(&["settle"]);

    let out = s.answering(&["redact", "trip"], "no\n").1;
    assert!(out.contains("will show broken"), "{out}");
    assert!(
        out.contains(&linking),
        "the linking note was not named: {out}"
    );
    assert!(out.contains("filed under nothing"), "{out}");
    assert!(
        out.contains("trip-packing") || out.contains("packing"),
        "the child was not named: {out}"
    );
}
