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
