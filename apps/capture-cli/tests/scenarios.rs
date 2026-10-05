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

fn capture() -> &'static str {
    env!("CARGO_BIN_EXE_capture")
}

struct Store(PathBuf);

impl Store {
    fn new(name: &str) -> Store {
        let d = std::env::temp_dir().join(format!("capture-scn-{name}-{}", std::process::id()));
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

    /// Run a command at a given moment.
    ///
    /// `CAPTURE_NOW_MS` is honoured only in a debug build, which is the binary
    /// a test runs. An installed release ignores it.
    fn at(&self, when: &str, args: &[&str]) -> String {
        let ms = when
            .parse::<jiff::Timestamp>()
            .expect("a timestamp")
            .as_millisecond();
        let out = Command::new(capture())
            .args(args)
            .env("CAPTURE_HOME", &self.0)
            .env("CAPTURE_NOW_MS", ms.to_string())
            .output()
            .expect("capture runs");
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success(), "capture {args:?} at {when}: {text}");
        text
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
        said.contains("capture + stauts"),
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
    // build cannot fold, must both verify clean — otherwise `capture verify`
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
    // It reads counters and opens no event, so it names `capture verify` rather
    // than implying the archive is intact.
    let s = Store::new("status-honest");
    s.capture("something worth keeping around");

    let said = s.ok(&["status"]);
    assert!(said.contains("Not checked here"), "{said}");
    assert!(said.contains("capture verify"), "{said}");
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

// ------------------------------------------------------------------ split ----

#[test]
fn a_series_can_be_changed_from_one_occurrence_onward() {
    // §3.5, and until now a store function with nothing able to call it. The
    // gate that looks for public items with no caller is what found it.
    let s = Store::new("split-series");
    s.ok(&["+", "standup every weekday at 9"]);
    // Promotion is a sweep that runs on any writing session but not on a
    // capture's, so the series does not exist until something else writes.
    s.ok(&["+", "anything at all"]);

    let timeline = s.ok(&["timeline", "--json"]);
    let slug = timeline
        .split('"')
        .find(|w| w.starts_with("standup-every-weekday-"))
        .expect("the series was captured")
        .to_string();

    let out = s.ok(&[
        "split",
        &slug,
        "--from",
        "2026-10-01",
        "--to",
        "every tuesday at 9",
    ]);
    assert!(out.contains("keep the old rule"), "{out}");

    // Two notes with the same text: the original, bounded, and the new rule
    // from the boundary on. That is what a split is, and the message says so
    // because otherwise it reads as a duplicate.
    let after = s.ok(&["timeline", "--json"]);
    assert_eq!(
        after.matches("standup-every-weekday-").count(),
        2,
        "expected the original and its continuation: {after}"
    );
}

#[test]
fn splitting_on_something_that_is_not_a_rule_says_so() {
    // The rule is checked before the store is opened. When the checks ran the
    // other way round, a bad rule on an already-split series was reported as
    // "no occurrence after that point", which sends the reader to the wrong
    // problem entirely.
    let s = Store::new("split-bad-rule");
    s.ok(&["+", "standup every weekday at 9"]);
    s.ok(&["+", "anything at all"]);
    let timeline = s.ok(&["timeline", "--json"]);
    let slug = timeline
        .split('"')
        .find(|w| w.starts_with("standup-every-weekday-"))
        .expect("captured")
        .to_string();

    let (code, out) = s.run(&["split", &slug, "--from", "2026-10-01", "--to", "buy milk"]);
    assert!(code != 0, "a non-rule was accepted");
    assert!(out.contains("not a repeating rule"), "{out}");
}

/// A finished task leaves the listing, and the listing is banded.
///
/// `read::tasks` called `promoted("task", 500)` — every task promotion ever
/// made, flat and unbanded — rather than `tasks_outstanding`, which bands them
/// and drops the finished ones. So completing a task left it on screen reading
/// "done · due 2026-09-20", and the three bands the requirement names were
/// absent entirely.
///
/// The store's own test asserted all of this of `tasks_outstanding` the whole
/// time. The command simply was not asking `tasks_outstanding`, and nothing
/// checked what the command did.
#[test]
fn completing_a_task_takes_it_out_of_the_listing() {
    let s = Store::new("tasks-banded");

    let overdue = s.capture("pay the invoice");
    let later = s.capture("call the plumber");
    let undated = s.capture("send sarah the migration doc");
    s.ok(&["settle"]);
    s.ok(&["task", &overdue, "2026-09-20"]);
    s.ok(&["task", &later, "2036-12-01"]);
    s.ok(&["task", &undated]);

    let listed = s.ok(&["tasks"]);
    for band in ["overdue", "due", "no date"] {
        assert!(
            listed.contains(band),
            "the {band} band is missing:\n{listed}"
        );
    }
    assert!(listed.contains(&overdue), "the overdue task is missing");

    // The one that was wrong: a finished task stays listed.
    s.ok(&["done", &overdue]);
    let after = s.ok(&["tasks"]);
    assert!(
        !after.contains(&overdue),
        "a finished task is still outstanding:\n{after}"
    );
    assert!(
        !after.contains("overdue"),
        "an empty band is still drawn:\n{after}"
    );
    assert!(after.contains(&later), "an open task was dropped too");

    // `band` and `due` ride along, because a consumer handed only a descriptor
    // has to parse prose to learn when something is due.
    let json = s.ok(&["tasks", "--json"]);
    assert!(json.contains("\"band\":\"due\""), "{json}");
    assert!(json.contains("\"due\":\"2036-12-01\""), "{json}");
    assert!(json.contains("\"band\":\"undated\""), "{json}");

    s.ok(&["done", &later]);
    s.ok(&["done", &undated]);
    assert!(
        s.ok(&["tasks"]).contains("Nothing outstanding"),
        "an empty surface should say so plainly"
    );
}

/// An agenda asked for as JSON says what it has not read.
///
/// The requirement names three commands — agenda, tasks and occurrences — and
/// only `tasks --json` ever carried the count. `agenda --json` answered a bare
/// array and returned before every one of the five `say_if_behind` call sites,
/// all of which are on the prose path.
///
/// So the same question asked two ways was hedged in a terminal and confident
/// in a script, and the script is the consumer least able to notice: a person
/// reading "nothing on Tuesday" sees the line underneath it, a program
/// receiving `[]` sees an empty day.
#[test]
fn an_agenda_in_json_reports_what_is_unread() {
    let s = Store::new("agenda-unread");

    // Read, so the count is a real zero rather than an absence.
    s.capture("dentist thursday 2pm");
    s.ok(&["settle"]);
    let settled = s.ok(&["agenda", "--json"]);
    assert!(
        settled.contains("\"unread\":0"),
        "the field should be present at zero, so the shape does not depend on \
         the state:\n{settled}"
    );

    // Captured and deliberately not settled.
    s.capture("pay the invoice");
    s.capture("call the plumber");
    let behind = s.ok(&["agenda", "--json"]);
    assert!(
        behind.contains("\"unread\":2"),
        "an answer derived from incomplete promotion was presented as a \
         complete one:\n{behind}"
    );

    // And it still answers the question it was asked.
    assert!(
        behind.contains("\"entries\":"),
        "the entries went missing:\n{behind}"
    );
}

/// The overdue boundary is asked against the clock, and a test can move it.
///
/// `CAPTURE_NOW_MS` exists because fourteen places read the wall clock directly
/// and nothing could move any of them, so everything decided against "now" was
/// only ever exercised at whatever moment the suite happened to run.
///
/// A task due on a fixed date is overdue or not depending only on when you ask.
/// Before this there was no way to ask twice.
#[test]
fn a_task_is_overdue_or_not_depending_on_when_you_ask() {
    let s = Store::new("overdue-clock");
    let slug = s.capture("pay the invoice");
    s.ok(&["settle"]);
    s.ok(&["task", &slug, "2026-10-05"]);

    // The day before: due, not overdue.
    let before = s.at("2026-10-04T12:00:00Z", &["tasks"]);
    assert!(before.contains("due"), "{before}");
    assert!(
        !before.contains("overdue"),
        "a task due tomorrow was called overdue: {before}"
    );

    // The day itself: still not overdue. A task due today is not late, which is
    // the boundary a handful of hand-picked dates agrees with an off-by-one as
    // readily as with the fix.
    let on = s.at("2026-10-05T12:00:00Z", &["tasks"]);
    assert!(
        !on.contains("overdue"),
        "a task due today was called overdue: {on}"
    );

    // The day after.
    let after = s.at("2026-10-06T12:00:00Z", &["tasks"]);
    assert!(
        after.contains("overdue"),
        "a task due yesterday is not: {after}"
    );
}

/// Retiring refuses the two things that would leave the archive unrecoverable.
///
/// `capture retire` is run by no test. Both halves of what it does are
/// dangerous — it revokes a key, and `--yes` is the only thing between a typo
/// and doing so — and the guards in front of it were unexercised.
///
/// A roster of two needs two devices, which a single-process test cannot build.
/// The refusals do not: they are reachable on a store that has just been
/// created, and they are the ones worth having, because each prevents a state
/// nothing can undo.
#[test]
fn retiring_refuses_what_cannot_be_undone() {
    let s = Store::new("retire-guards");
    s.capture("something to prove the archive still works afterwards");
    s.ok(&["settle"]);

    let mine = s.ok(&["peers", "--json"]);

    // A key nobody has. Not an error about retirement — an error about the
    // name, because that is what is wrong.
    let (code, said) = s.run(&["retire", "ffffffff"]);
    assert_ne!(code, 0, "retiring a device that does not exist succeeded");
    assert!(
        said.contains("no device in the roster matches"),
        "the refusal should be about the name: {said}"
    );

    // This device, when it is the only one. The roster would be left with
    // nobody, and `device_add` needs a current member's signature — so there
    // would be no way back in, ever.
    let own = own_key(&mine);
    let (code, said) = s.run(&["retire", &own]);
    assert_ne!(code, 0, "retiring the only device in the roster succeeded");
    assert!(
        said.contains("never admit anyone again"),
        "the refusal should say why it cannot be undone: {said}"
    );

    // And nothing was written by either attempt. A refusal that still wrote an
    // event would leave a retirement in the log that the roster then has to
    // argue with.
    assert_eq!(
        s.ok(&["peers", "--json"]),
        mine,
        "a refused retirement changed the roster"
    );
}

/// The first eight hex characters of this device's key, as `peers --json` gives
/// them.
fn own_key(json: &str) -> String {
    let at = json.find("\"key\":\"").expect("peers names a key") + 7;
    json[at..].chars().take(8).collect()
}
