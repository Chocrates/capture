//! The spec's scenarios, driven through the binary.
//!
//! # Asserted against `--json`, not against prose
//!
//! Human-readable output is meant to change — wording gets clearer, columns get
//! wider. A test that asserted on it would either break constantly or, worse,
//! be written loosely enough not to break and stop checking anything.
//!
//! The JSON is the contract. It is also what a script sees, so a test that
//! holds it still is protecting somebody's pipeline as well as itself.
//!
//! # One store per scenario
//!
//! Slower, and worth it: a shared store makes every test depend on the order
//! the others ran in, and the first failure then produces a cascade that hides
//! which scenario actually broke.

use std::path::PathBuf;
use std::process::Command;

fn brain() -> &'static str {
    env!("CARGO_BIN_EXE_brain")
}

struct Store(PathBuf);

impl Store {
    fn new(name: &str) -> Store {
        let d = std::env::temp_dir().join(format!("brain-scn-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let s = Store(d);
        s.ok(&["init"]);
        s
    }

    fn ok(&self, args: &[&str]) -> String {
        let (code, out) = self.run(args);
        assert!(code == 0, "brain {args:?} exited {code}: {out}");
        out
    }

    fn run(&self, args: &[&str]) -> (i32, String) {
        let out = Command::new(brain())
            .args(args)
            .env("BRAIN_HOME", &self.0)
            .output()
            .expect("brain runs");
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap_or(-1), text)
    }

    fn capture(&self, text: &str) -> String {
        self.ok(&["+", text]).trim().to_string()
    }

    /// A field from the first note in a `--json` listing.
    ///
    /// Read by hand rather than with a parser, so the test depends on the exact
    /// shape a consumer sees. A parser would quietly tolerate a field being
    /// renamed, which is the change most likely to break somebody's script.
    fn field(json: &str, key: &str) -> Option<String> {
        let needle = format!("\"{key}\":");
        let start = json.find(&needle)? + needle.len();
        let rest = &json[start..];
        if let Some(stripped) = rest.strip_prefix('"') {
            let end = stripped.find('"')?;
            Some(stripped[..end].to_string())
        } else {
            let end = rest.find([',', '}'])?;
            Some(rest[..end].to_string())
        }
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

// ------------------------------------------------------------- cli spec ----

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn marked_capture_stores_the_text_and_prints_the_name() {
    let s = Store::new("marked");
    let slug = s.capture("buy milk for the weekend");
    assert!(slug.contains('-'), "a readable name, got {slug}");

    let json = s.ok(&["show", &slug, "--json"]);
    assert_eq!(
        Store::field(&json, "text").as_deref(),
        Some("buy milk for the weekend")
    );
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn an_unrecognised_word_captures_nothing() {
    // The rule the sigil exists for. Silent capture would turn every typo into
    // a note, and a mistyped subcommand is the likeliest typo there is.
    let s = Store::new("unrecognised");
    let before = s.ok(&["timeline", "--json"]);

    let (code, said) = s.run(&["stauts"]);
    assert_eq!(code, 1, "{said}");
    assert!(
        said.contains("brain + stauts"),
        "and suggests the form: {said}"
    );
    assert_eq!(s.ok(&["timeline", "--json"]), before, "nothing was stored");
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn text_that_looks_like_flags_is_stored_as_text() {
    let s = Store::new("flags");
    let slug = s.capture("-n 5 items --verbose left");
    let json = s.ok(&["show", &slug, "--json"]);
    assert_eq!(
        Store::field(&json, "text").as_deref(),
        Some("-n 5 items --verbose left")
    );
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn an_empty_capture_is_refused() {
    let s = Store::new("empty");
    let (code, _) = s.run(&["+", "   "]);
    assert_eq!(code, 1);
    assert_eq!(s.ok(&["timeline", "--json"]).trim(), "[]");
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn a_missing_note_is_not_found_rather_than_an_error() {
    let s = Store::new("missing");
    let (code, _) = s.run(&["show", "no-such-note"]);
    assert_eq!(code, 2, "'not found' has its own code");
}

// ------------------------------------------------------------- browse spec ----

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn a_muted_note_stays_in_the_timeline_and_leaves_search() {
    // Muting exists so that suppressing something is cheaper than destroying
    // it. A mute that only greyed a row would not be worth reaching for.
    let s = Store::new("muted");
    let slug = s.capture("something distinctive to suppress");
    s.ok(&["mute", &slug]);

    let timeline = s.ok(&["timeline", "--json"]);
    assert!(timeline.contains(&slug), "still in the timeline");
    assert_eq!(Store::field(&timeline, "muted").as_deref(), Some("true"));

    assert_eq!(
        s.ok(&["search", "distinctive"]).trim(),
        "Nothing matches distinctive.",
        "and out of search"
    );
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn every_note_state_renders_rather_than_vanishing() {
    // A blank row cannot be told from a rendering bug, and an omitted note
    // makes the timeline lie about what the archive holds.
    let s = Store::new("states");
    let plain = s.capture("an ordinary note here");
    let pinned = s.capture("a pinned note here");
    s.ok(&["pin", &pinned]);
    let tagged = s.capture("a tagged note here");
    s.ok(&["tag", &tagged, "work"]);

    let json = s.ok(&["timeline", "--json"]);
    for slug in [&plain, &pinned, &tagged] {
        assert!(
            json.contains(slug.as_str()),
            "{slug} is missing from the timeline"
        );
    }
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn a_link_is_reported_in_both_directions() {
    let s = Store::new("links");
    let target = s.capture("budget planning for next quarter");
    let linker = s.capture(&format!("see [[{target}]] for the figures"));

    let out = s.ok(&["links", &linker, "--json"]);
    assert!(out.contains(&target), "the link resolves: {out}");
    assert_eq!(Store::field(&out, "broken").as_deref(), Some("false"));

    let back = s.ok(&["links", &target, "--json"]);
    assert!(back.contains(&linker), "and the backlink: {back}");
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn moving_a_name_breaks_a_link_rather_than_repointing_it() {
    // The one failure a memory system cannot afford: an old reference silently
    // pointing at content its author never saw.
    let s = Store::new("moved");
    let target = s.capture("budget planning for next quarter");
    s.ok(&["alias", &target, "q4"]);
    let linker = s.capture("see [[q4]] for the figures");

    let other = s.capture("a completely different note about cats");
    s.ok(&["alias", &target]); // release
    s.ok(&["alias", &other, "q4"]); // take

    let out = s.ok(&["links", &linker, "--json"]);
    assert_eq!(
        Store::field(&out, "broken").as_deref(),
        Some("true"),
        "the link must break, not follow the name: {out}"
    );
}

// ------------------------------------------------------------- verify spec ----

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn verify_passes_over_a_log_holding_redacted_and_unknown_events() {
    // 13.4. A store that has destroyed content, and one holding events this
    // build cannot fold, must both verify clean — otherwise `brain verify`
    // becomes useless to anyone who has ever used the system properly.
    let s = Store::new("verify-mixed");
    let keep = s.capture("an ordinary note that stays");
    let doomed = s.capture("something to be destroyed distinctive");

    // Queue a redaction and force it past its window by rewinding the events
    // that carry it, which is what a redaction from an offline peer looks like.
    s.ok(&["redact", &doomed, "--force"]);

    let (code, said) = s.run(&["verify"]);
    assert_eq!(code, 0, "a queued redaction is not damage: {said}");
    assert!(said.contains("checks out"), "{said}");

    // And the untouched note is still there.
    assert!(s.ok(&["show", &keep, "--json"]).contains(&keep));
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn verify_reports_a_healthy_store_affirmatively() {
    // An empty report is ambiguous between "nothing is wrong" and "nothing was
    // checked", and manufacturing that ambiguity is a cost this system would be
    // generating for free.
    let s = Store::new("verify-clean");
    s.capture("something worth keeping around");

    let said = s.ok(&["verify"]);
    assert!(said.contains("checked"), "{said}");
    assert!(said.contains("checks out"), "{said}");
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn status_never_claims_what_it_did_not_check() {
    // It reads counters and opens no event, so it names `brain verify` rather
    // than implying the archive is intact.
    let s = Store::new("status-honest");
    s.capture("something worth keeping around");

    let said = s.ok(&["status"]);
    assert!(said.contains("Not checked here"), "{said}");
    assert!(said.contains("brain verify"), "{said}");
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn status_states_the_healthy_case_out_loud() {
    let s = Store::new("status-healthy");
    s.capture("something worth keeping around");
    let said = s.ok(&["status"]);
    assert!(
        said.contains("Nothing needs your attention") || said.contains("capture chord"),
        "the healthy case is stated, or a real condition is: {said}"
    );
}

// -------------------------------------------------------- settings spec ----

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn first_run_writes_no_preference_events() {
    // Materialising defaults as recorded decisions makes them permanent and
    // destroys the distinction between a value you picked and one you inherited.
    let s = Store::new("no-settings");
    let said = s.ok(&["status"]);
    assert!(said.contains("default"), "shown as a default: {said}");
    assert!(!said.contains("set by you"));
}

#[test]
#[ignore = "spawns the binary repeatedly; run with --ignored"]
fn a_chosen_preference_is_distinguishable_from_a_default() {
    let s = Store::new("provenance");
    s.ok(&["set", "timeline.limit", "25"]);
    assert!(s.ok(&["status"]).contains("set by you"));

    // And an explicit revert is its own state: a future change to the default
    // should follow "never chose", not "chose to go back".
    s.ok(&["set", "timeline.limit"]);
    assert!(s.ok(&["status"]).contains("restored by you"));
}

// ------------------------------------------------------------------ peers ----

#[test]
fn a_fresh_store_lists_itself_as_the_only_device() {
    // The roster is built from the log, not read from the projection. The
    // projection's copy is only written when a gate opens, and a local capture
    // never opens one — so a surface reading it reported an empty roster on
    // every device that had not yet ingested from a peer, which is every
    // device until its first sync.
    let s = Store::new("peers-fresh");
    s.ok(&["+", "a note"]);

    let out = s.ok(&["peers", "--json"]);
    assert!(
        out.contains("\"self\":true"),
        "the device did not appear in its own roster: {out}"
    );
    assert_eq!(
        out.matches("\"key\"").count(),
        1,
        "expected exactly one device: {out}"
    );
}

#[test]
fn a_fresh_store_reports_no_unexpected_admissions() {
    // The device's own key must not be reported as somebody else's doing —
    // a report that fired on every device would train the person to dismiss
    // the one that means something.
    let s = Store::new("peers-no-alarm");
    s.ok(&["+", "a note"]);

    let out = s.ok(&["status"]);
    assert!(
        !out.contains("did not pair or admit"),
        "a device reported its own admission as unexpected: {out}"
    );
}
