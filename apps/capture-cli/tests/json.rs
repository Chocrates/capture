//! Every `--json` output parses, and carries what the prose says.
//!
//! # Parsed, unlike `scenarios.rs`
//!
//! The scenarios read JSON by hand so that they depend on the exact shape a
//! consumer sees. This file asks two different questions, and both need a real
//! parser: is the output JSON at all, for a note holding everything capture is
//! allowed to hold verbatim — and is a given fact *somewhere* in it.
//!
//! # Why the second question
//!
//! The prose and JSON renderings were written separately, and the JSON is the
//! one nobody looks at while writing a command. It fell behind four times before
//! the renderers were made to share one value; the tests below are the cases the
//! audit that prompted that found still behind.

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;

/// The moment every command in this file runs at. Fixed, so "overdue" and
/// "owed" mean the same thing on every run.
const NOW: &str = "2026-10-10T12:00:00Z";

struct Store(PathBuf);

impl Store {
    fn new(name: &str) -> Store {
        let d = std::env::temp_dir().join(format!("capture-json-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let s = Store(d);
        s.at(NOW, &["init"]);
        s
    }

    /// Run a command at a moment, in UTC, and require it to succeed.
    fn at(&self, when: &str, args: &[&str]) -> String {
        let ms = when
            .parse::<jiff::Timestamp>()
            .expect("a timestamp")
            .as_millisecond();
        let out = Command::new(env!("CARGO_BIN_EXE_capture"))
            .args(args)
            .env("CAPTURE_HOME", &self.0)
            .env("CAPTURE_NOW_MS", ms.to_string())
            .env("TZ", "UTC")
            .output()
            .expect("capture runs");
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        assert!(
            out.status.success(),
            "capture {args:?} at {when}: {text}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        text
    }

    fn ok(&self, args: &[&str]) -> String {
        self.at(NOW, args)
    }

    fn capture(&self, text: &str) -> String {
        self.ok(&["+", text]).trim().to_string()
    }

    /// A command's `--json`, parsed, or a failure naming the command and what
    /// it printed.
    fn json_at(&self, when: &str, args: &[&str]) -> Value {
        let mut all = args.to_vec();
        all.push("--json");
        let out = self.at(when, &all);
        serde_json::from_str(&out).unwrap_or_else(|e| {
            panic!("capture {all:?} printed something that is not JSON ({e}):\n{out}")
        })
    }

    fn json(&self, args: &[&str]) -> Value {
        self.json_at(NOW, args)
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// The entry in an array whose `slug` is the one given.
fn by_slug<'a>(rows: &'a Value, slug: &str) -> Option<&'a Value> {
    rows.as_array()?.iter().find(|r| r["slug"] == slug)
}

// ------------------------------------------------------------- parsing ----

/// Every command that takes `--json`, over an archive with something of each
/// kind in it and a note holding the characters most likely to break a string.
#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn every_json_output_parses() {
    let s = Store::new("parses");

    // Capture is verbatim, so a note may hold anything. A quote, a backslash,
    // a newline, a tab and a bell are each a different way to end a string
    // early or produce one a parser refuses.
    let hostile = s.capture("say \"hi\" \\ then\nnext line\twith a bell \u{7} @sarah #shop");
    let task = s.capture("pay the invoice");
    let event = s.capture("dentist tomorrow at 2pm");
    let series = s.capture("standup every day at 9am");
    s.capture("#shop: milk, eggs, bread");
    let linker = s.capture(&format!("see [[{hostile}]] and ^acme"));
    s.ok(&["settle"]);
    s.ok(&["task", &task, "2026-10-01"]);
    s.ok(&["mute", &hostile, "until", "2027-01-01"]);
    s.ok(&["intent", "at", "2026-10-11T09:00", "ring \"the\" dentist"]);

    let commands: Vec<Vec<&str>> = vec![
        vec!["timeline"],
        vec!["show", &hostile],
        vec!["show", &task],
        vec!["agenda"],
        vec!["agenda", "2026-10-11"],
        vec!["tasks"],
        vec!["list", "shop"],
        vec!["due"],
        vec!["mentions", "@sarah"],
        vec!["mentions", "#shop"],
        vec!["people"],
        vec!["projects"],
        vec!["entity", "@sarah"],
        vec!["entity", "@nobody"],
        vec!["review"],
        vec!["intents"],
        vec!["during", &series],
        vec!["search", "invoice"],
        vec!["links", &linker],
        vec!["links", &hostile],
        vec!["log", &event],
        vec!["quarantine"],
        vec!["held"],
        vec!["rejected"],
        vec!["verify"],
        vec!["peers"],
        vec!["recover"],
        vec!["settle"],
        vec!["export"],
        vec!["export"],
    ];
    for c in &commands {
        s.json(c);
    }

    // And the hostile text came back as it went in, not merely as something
    // that parses.
    let shown = s.json(&["show", &hostile]);
    assert_eq!(
        shown["text"],
        "say \"hi\" \\ then\nnext line\twith a bell \u{7} @sarah #shop"
    );
}

// --------------------------------------------------------------- drift ----

/// The agenda's "Still open" and "Still owed" reach the JSON.
///
/// Both were printed in prose and absent from `agenda --json`, so a script
/// asking what today holds was told about today's appointments and none of
/// the work carried into it.
#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn the_agendas_carried_work_reaches_the_json() {
    let s = Store::new("agenda-carried");

    let task = s.capture("pay the invoice");
    s.ok(&["settle"]);
    s.ok(&["task", &task, "2026-10-01"]);

    // A daily series, which somebody said accumulates, read three days on with
    // nothing marked done — so the days between are owed.
    //
    // Three days on from the *real* clock, not from `NOW`. `CAPTURE_NOW_MS`
    // moves what a command takes "today" to be; the event's own stamp comes
    // from the store's hybrid clock, which follows the wall. A series captured
    // "at" an earlier moment still begins now.
    let series = s.capture("take the pill every day at 9");
    s.ok(&["settle"]);
    s.ok(&["carries-forward", &series]);
    let later = (jiff::Timestamp::now() + jiff::SignedDuration::from_hours(72)).to_string();

    // The prose says both, which is what the JSON is being held to.
    let prose = s.at(&later, &["agenda"]);
    assert!(prose.contains("Still open"), "{prose}");
    assert!(prose.contains("Still owed"), "{prose}");

    let json = s.json_at(&later, &["agenda"]);
    let overdue = by_slug(&json["overdue"], &task)
        .unwrap_or_else(|| panic!("the overdue task is not in the JSON: {json}"));
    assert_eq!(overdue["due"], "2026-10-01");
    assert!(
        by_slug(&json["owed"], &series).is_some(),
        "the owed occurrence is not in the JSON: {json}"
    );

    // Every entry says whether it was cancelled, because the prose does.
    for e in json["entries"].as_array().unwrap() {
        assert!(e.get("cancelled").is_some(), "no `cancelled` on {e}");
    }
}

/// `due --json` says what it will do about each thing, which is the question
/// the command exists to answer.
#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn the_due_list_says_what_it_will_do() {
    let s = Store::new("due-state");
    let event = s.capture("dentist tomorrow at 2pm");
    s.ok(&["settle"]);

    let json = s.json(&["due"]);
    let rows = json["due"]
        .as_array()
        .unwrap_or_else(|| panic!("no `due` array: {json}"));
    assert_eq!(rows.len(), 1, "{json}");
    assert_eq!(rows[0]["state"], "waiting", "{json}");
    assert_eq!(rows[0]["slug"], event.as_str(), "{json}");
    assert_eq!(json["unread"], 0, "{json}");
    assert!(json["unsettled"].is_null(), "{json}");

    // Unread captures are reported, as the prose reports them.
    s.capture("dentist friday at 3pm");
    assert_eq!(s.json(&["agenda"])["unread"], 1);
}

/// A note's page in JSON says what it was recognised as, and what it links to.
#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn a_notes_page_in_json_says_what_it_was_recognised_as() {
    let s = Store::new("show-promotion");
    let task = s.capture("send sarah the migration doc tomorrow");
    let linker = s.capture(&format!("about [[{task}]]"));
    let plain = s.capture("an ordinary thought");
    s.ok(&["settle"]);

    let page = s.json(&["show", &task]);
    assert_eq!(page["promotion"]["kind"], "task", "{page}");
    assert!(
        page["promotion"]["descriptor"]
            .as_str()
            .is_some_and(|d| d.contains("due")),
        "{page}"
    );
    assert_eq!(page["promotion"]["authored"], false, "{page}");
    assert!(
        by_slug(&page["backlinks"], &linker).is_some(),
        "the backlink the prose prints is missing: {page}"
    );

    // A plain note has no promotion line in prose, and null here.
    assert!(s.json(&["show", &plain])["promotion"].is_null());
}

/// A mute that has ended is not reported as a mute.
///
/// `"muted"` was `muted_until.is_some()`, so it stayed true for ever — the
/// mistake the unmute change removed from ten SQL predicates, still present in
/// the one place a script would look.
#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn a_lapsed_mute_is_not_reported_as_a_mute() {
    let s = Store::new("mute-lapse");
    let n = s.capture("the thing");
    s.ok(&["settle"]);
    s.ok(&["mute", &n, "until", "2027-03-01"]);

    let before = s.json_at("2027-02-01T12:00:00Z", &["show", &n]);
    assert_eq!(before["muted"], true, "{before}");
    assert!(
        before["muted_until"].as_i64().is_some_and(|t| t > 0),
        "{before}"
    );

    let after = s.json_at("2027-03-02T12:00:00Z", &["timeline"]);
    let row = by_slug(&after, &n).expect("the note is listed");
    assert_eq!(
        row["muted"], false,
        "a lapsed mute is still reported: {row}"
    );
    assert_eq!(row["muted_until"], before["muted_until"], "{row}");

    // And an indefinite one says so with zero, as the prose says "indefinitely".
    s.ok(&["mute", &n]);
    assert_eq!(s.json(&["show", &n])["muted_until"], 0);
}

/// What the other listings print in prose and did not carry.
#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn the_other_listings_carry_what_their_prose_says() {
    let s = Store::new("other-drift");
    let task = s.capture("pay the invoice");
    let queued = s.capture("a password: hunter2");
    s.ok(&["settle"]);
    s.ok(&["task", &task, "2026-10-20"]);

    // Tasks print the note's text beside the descriptor.
    let tasks = s.json(&["tasks"]);
    let row = by_slug(&tasks["tasks"], &task).expect("the task is listed");
    assert_eq!(row["text"], "pay the invoice", "{tasks}");

    // A note queued for redaction is withheld, not textless, and the prose
    // says which. Queued and not yet destroyed, because the window is open.
    s.ok(&["redact", &queued, "--force"]);
    let row = s.json(&["timeline"]);
    let row = by_slug(&row, &queued).expect("still listed");
    assert_eq!(row["redaction_queued"], true, "{row}");

    // Review: every item carries the suggestion slot the prose prints, and the
    // answer carries the lapsed-series prompt the prose asks.
    let review = s.json(&["review"]);
    for i in review["items"]
        .as_array()
        .unwrap_or_else(|| panic!("{review}"))
    {
        assert!(i.get("suggestion").is_some(), "{i}");
    }
    assert!(review["lapsed"].is_array(), "{review}");

    // Peers: the sync sentence under the rows.
    let peers = s.json(&["peers"]);
    assert!(peers["peers"].is_array(), "{peers}");
    assert!(peers.get("sync").is_some(), "{peers}");

    // Export: what it withheld and which notes it stripped.
    let export = s.json(&["export"]);
    assert!(export["withheld"].is_number(), "{export}");
    assert!(export["flagged"].is_array(), "{export}");
}
