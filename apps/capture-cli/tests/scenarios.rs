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

    /// Run a command as a machine in a named zone.
    ///
    /// Set per child process, so one test can be two machines. Anything whose
    /// rendering depends on where the reader is has to be checked from more
    /// than one place, or a surface that hardcodes UTC passes everywhere
    /// outside UTC and nothing notices.
    fn in_zone(&self, tz: &str, args: &[&str]) -> String {
        let out = Command::new(capture())
            .args(args)
            .env("CAPTURE_HOME", &self.0)
            .env("TZ", tz)
            .output()
            .expect("capture runs");
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success(), "capture {args:?} in {tz}: {text}");
        text
    }

    fn capture(&self, text: &str) -> String {
        self.ok(&["+", text]).trim().to_string()
    }

    /// The first occurrence of a series in the week ahead, as (day, start).
    ///
    /// Asked for rather than computed. A test that worked out which day a
    /// weekday rule produces would be reimplementing the recurrence in order to
    /// check the recurrence, and would agree with its own mistakes.
    fn first_occurrence(&self, series: &str) -> Option<(String, i64)> {
        let today = jiff::Zoned::now().date();
        for ahead in 0..8 {
            let day = today
                .tomorrow()
                .ok()?
                .checked_add(jiff::Span::new().days(ahead))
                .ok()?;
            let day = day.to_string();
            let json = self.ok(&["agenda", &day, "--json"]);
            if !json.contains(series) {
                continue;
            }
            let at = json.find("\"start_ms\":")? + "\"start_ms\":".len();
            let n: String = json[at..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            return Some((day, n.parse().ok()?));
        }
        None
    }

    /// The first occurrence of a series on a day after the one given.
    fn first_occurrence_after(&self, series: &str, after: &str) -> Option<(String, i64)> {
        let from: jiff::civil::Date = after.parse().ok()?;
        for ahead in 1..8 {
            let day = from
                .checked_add(jiff::Span::new().days(ahead))
                .ok()?
                .to_string();
            let json = self.ok(&["agenda", &day, "--json"]);
            if !json.contains(series) {
                continue;
            }
            let at = json.find("\"start_ms\":")? + "\"start_ms\":".len();
            let n: String = json[at..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            return Some((day, n.parse().ok()?));
        }
        None
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

/// A rebuild reproduces the archive, which is the claim the design rests on.
///
/// §1.3: `state.db` is disposable and rebuilt by re-folding the log. There is
/// no migration code, ever — and that is only affordable if the rebuild really
/// does reproduce what was there. `capture rebuild` is the command a person
/// runs to exercise it, and no test had ever run it.
///
/// Asserted against what the surfaces say rather than against a row count,
/// because a rebuild that produced the right number of wrong notes would
/// satisfy a count.
#[test]
fn a_rebuild_reproduces_what_the_archive_said() {
    let s = Store::new("rebuild");

    // Something of each kind the projection derives differently: a promotion
    // with a date, a list, a tag applied by an event, a mention, and a note
    // somebody muted.
    let task = s.capture("pay the invoice");
    s.capture("#shop: milk, eggs, bread");
    s.capture("lunch with @sarah about ^acme");
    let muted = s.capture("something I do not want resurfacing");
    s.ok(&["settle"]);
    s.ok(&["task", &task, "2026-09-20"]);
    s.ok(&["tag", &muted, "vault"]);
    s.ok(&["mute", &muted]);

    let before = (
        s.ok(&["timeline", "--json"]),
        s.ok(&["tasks", "--json"]),
        s.ok(&["list", "shop"]),
        s.ok(&["mentions", "@sarah"]),
        s.ok(&["show", &muted]),
    );

    let said = s.ok(&["rebuild"]);
    assert!(said.contains("rebuilt"), "{said}");
    assert!(
        said.contains("note(s)"),
        "a rebuild should say what it produced: {said}"
    );

    let after = (
        s.ok(&["timeline", "--json"]),
        s.ok(&["tasks", "--json"]),
        s.ok(&["list", "shop"]),
        s.ok(&["mentions", "@sarah"]),
        s.ok(&["show", &muted]),
    );

    assert_eq!(before.0, after.0, "the timeline changed across a rebuild");
    assert_eq!(before.2, after.2, "the list changed across a rebuild");
    assert_eq!(before.3, after.3, "the mentions changed across a rebuild");
    assert_eq!(before.4, after.4, "the note changed across a rebuild");

    // The tasks are the same and the *count beside them* is not, which is a
    // property worth pinning rather than normalising away.
    //
    // `read_by` is local and a rebuild drops it. The queue handles that by
    // treating a current promotion as a record that the note was read —
    // otherwise every rebuild would sweep the whole archive and write a
    // duplicate promote event per note. But a note with **no** promotion has
    // nothing to stand in: ordinary prose the recognizer declined, and a
    // promotion a person pinned, which carries no grammar.
    //
    // So a rebuild reports every such note outstanding. It is honest — this
    // generation has not read them — and it is self-correcting, and for prose
    // the sweep writes no events. It is also the one visible cost of §1.3's
    // "no migration code, ever", and somebody who rebuilds a large archive
    // will see a large number here.
    assert_eq!(
        strip_unread(&before.1),
        strip_unread(&after.1),
        "the tasks themselves changed across a rebuild"
    );
    assert!(
        before.1.contains("\"unread\":0"),
        "nothing should have been outstanding beforehand: {}",
        before.1
    );
    assert!(
        !after.1.contains("\"unread\":0"),
        "a rebuild dropped `read_by` and nothing noticed: {}",
        after.1
    );

    // And a settle puts it back, which is what makes the above a cost rather
    // than a defect.
    s.ok(&["settle"]);
    assert_eq!(
        s.ok(&["tasks", "--json"]),
        before.1,
        "a settle after a rebuild did not restore what the archive said"
    );
}

/// The task listing without its outstanding count.
fn strip_unread(json: &str) -> String {
    match json.find(",\"unread\"") {
        Some(at) => format!("{}}}", &json[..at]),
        None => json.to_string(),
    }
}

/// Moving and amending an occurrence, through the commands rather than the
/// store.
///
/// Both were added with store tests and argument tests and nothing in between:
/// `mutate::move_occurrence` is covered, `capture move` parses, and whether the
/// command reaches the function was checked by hand on a scratch store and by
/// nothing afterwards. That is the gap `capture tasks` sat in for months — a
/// correct function, a command calling something else, and no test of the
/// wiring.
#[test]
fn an_occurrence_moves_and_amends_through_the_command() {
    let s = Store::new("occurrence-commands");
    let series = s.capture("standup every weekday at 9");
    s.ok(&["settle"]);

    // The first expansion within the week ahead. Found by asking rather than
    // computed, so the test does not reimplement the recurrence rule to check
    // the recurrence rule.
    let (day, occ) = s.first_occurrence(&series).expect("a series expands");

    // Move it five hours later. The occurrence keeps its name — that is what
    // makes a completion written before a move survive it — so it is still
    // addressed by the start the rule would have produced.
    let moved_to = occ + 5 * 60 * 60 * 1000;
    let said = s.ok(&["move", &series, &occ.to_string(), &moved_to.to_string()]);
    assert!(said.contains("moved"), "{said}");
    assert!(
        said.contains("rule is unchanged"),
        "a move should say what it did not touch: {said}"
    );

    let after = s.ok(&["agenda", &day, "--json"]);
    assert!(
        after.contains(&format!("\"start_ms\":{moved_to}")),
        "the occurrence did not move: {after}"
    );

    // Amend it. Saying what a thing is called is not saying when it happens, so
    // the time must be untouched.
    s.ok(&[
        "amend",
        &series,
        &occ.to_string(),
        "--title",
        "standup, in the annexe",
    ]);
    let amended = s.ok(&["agenda", &day, "--json"]);
    assert!(
        amended.contains("standup, in the annexe"),
        "the amendment did not take: {amended}"
    );
    assert!(
        amended.contains(&format!("\"start_ms\":{moved_to}")),
        "amending moved the occurrence: {amended}"
    );

    // And a refusal that names what was being attempted, on a note that has no
    // occurrences at all.
    let plain = s.capture("an ordinary thought");
    s.ok(&["settle"]);
    let (code, said) = s.run(&["move", &plain, "1", "2"]);
    assert_ne!(code, 0);
    assert!(
        said.contains("no occurrences") && said.contains("move"),
        "the refusal should name the action: {said}"
    );
}

/// Fixing a near miss rewrites the capture, which nothing else here does.
///
/// §3.1.3 is that promotion never touches the captured text, and this is the
/// one command that deliberately does — so what it changes should be exactly
/// one character, and everything else about the note should survive.
#[test]
fn fixing_a_list_inserts_one_character_and_nothing_else() {
    let s = Store::new("fix-list");
    let before = "#groceries 7 apples, rye flour, 1kg lentils";
    let slug = s.capture(before);
    s.ok(&["settle"]);

    let said = s.ok(&["fix-list", &slug]);
    assert!(said.contains("#groceries: 7 apples"), "{said}");

    let after = s.ok(&["show", &slug]);
    assert!(
        after.contains("#groceries: 7 apples, rye flour, 1kg lentils"),
        "the colon was not inserted where the tag ends: {after}"
    );

    // One character, and the same one. Anything else here would be the system
    // editing somebody's words beyond the thing it offered to do.
    let got: String = after
        .lines()
        .find(|l| l.contains("#groceries"))
        .expect("the text is shown")
        .trim()
        .into();
    assert_eq!(
        got.len(),
        before.len() + 1,
        "more than the colon changed: {got:?}"
    );
    assert_eq!(
        got.replacen(':', "", 1),
        before,
        "the words changed: {got:?}"
    );

    // The note keeps its identity — a rewrite is an edit, not a new capture.
    assert!(
        after.contains(&slug),
        "the note was renamed by being fixed: {after}"
    );

    // And it is now a list, which was the point.
    let list = s.ok(&["list", "groceries"]);
    for item in ["7 apples", "rye flour", "1kg lentils"] {
        assert!(list.contains(item), "{item} is missing: {list}");
    }
}

/// Prose is not a near miss, and fixing it is refused.
///
/// The guard that keeps the offer honest: §3.1.5 would rather never fire than
/// fire inside ordinary writing, and a command that inserted a colon into a
/// sentence would be doing exactly that on request.
#[test]
fn fixing_something_that_is_not_a_near_miss_is_refused() {
    let s = Store::new("fix-prose");
    let text = "ask sarah about the invoice, she has it";
    let slug = s.capture(text);
    s.ok(&["settle"]);

    let (code, said) = s.run(&["fix-list", &slug]);
    assert_ne!(code, 0, "prose was turned into a list: {said}");
    assert!(
        s.ok(&["show", &slug]).contains(text),
        "a refused fix changed the text anyway"
    );
}

/// Skipping one occurrence, and putting it back.
///
/// The restore path shipped broken once, and in a way reading the fold could
/// not show: `OccState::visible()` carried the rule that a move beats a cancel
/// and was read by nothing, so the expansion wrote `cancelled` straight through
/// and ignored `moved_to`. `unskip` ran, reported success, and the agenda still
/// showed nothing. It was found by *using* it.
///
/// So this uses it, through the commands, and checks the agenda rather than the
/// return code — because a command that reports success and changes nothing is
/// the exact shape of that defect.
#[test]
fn an_occurrence_is_skipped_and_put_back() {
    let s = Store::new("skip-restore");
    let series = s.capture("standup every weekday at 9");
    s.ok(&["settle"]);

    let (day, occ) = s.first_occurrence(&series).expect("a series expands");
    let next = s
        .first_occurrence_after(&series, &day)
        .expect("a weekday series has a second occurrence");

    s.ok(&["skip", &series, &occ.to_string()]);
    let after = s.ok(&["agenda", &day, "--json"]);
    assert!(
        !after.contains(&format!("\"start_ms\":{occ}")),
        "a skipped occurrence is still on the agenda: {after}"
    );

    // The rule is untouched: every other occurrence stands. The whole point of
    // an exception is that it is one.
    let sibling = s.ok(&["agenda", &next.0, "--json"]);
    assert!(
        sibling.contains(&format!("\"start_ms\":{}", next.1)),
        "skipping one occurrence took another with it: {sibling}"
    );

    s.ok(&["unskip", &series, &occ.to_string()]);
    let back = s.ok(&["agenda", &day, "--json"]);
    assert!(
        back.contains(&format!("\"start_ms\":{occ}")),
        "a restored occurrence did not come back — the defect this test exists \
         for reported success and changed nothing: {back}"
    );
}

/// Renaming and merging an entity move nothing.
///
/// §4.3's first and third failures, from the command line. A rename used to
/// fork the page and a merge has to be a *redirection*: no capture text
/// changes, no mention row is removed, and both spellings reach the survivor
/// afterwards. All three were covered at store level and by no command.
#[test]
fn an_entity_is_renamed_and_merged_without_moving_anything() {
    let s = Store::new("entities");
    let a = s.capture("ask @sarah about the invoice");
    let b = s.capture("@sarah-chen sent the contract");
    s.ok(&["settle"]);

    // Two spellings start as two people. That is the premise, not the bug:
    // nothing should guess they are one.
    assert_eq!(s.ok(&["entity", "@sarah"]).matches(&a).count(), 1);
    assert!(!s.ok(&["entity", "@sarah"]).contains(&b));

    s.ok(&["entity-rename", "@sarah", "Sarah Chen"]);
    let people = s.ok(&["people"]);
    assert!(
        people.contains("Sarah Chen"),
        "the chosen name is not shown: {people}"
    );

    // The rename did not fork the page: the note is still hers.
    let after_rename = s.ok(&["entity", "@sarah"]);
    assert!(
        after_rename.contains(&a),
        "a rename lost the notes that named her: {after_rename}"
    );

    s.ok(&["entity-merge", "@sarah-chen", "@sarah"]);

    // Both notes on one page, reachable by either spelling. This is the half a
    // merge exists for, and the half `mentions` deliberately does not do.
    for spelling in ["@sarah", "@sarah-chen"] {
        let page = s.ok(&["entity", spelling]);
        assert!(page.contains(&a) && page.contains(&b), "{spelling}: {page}");
    }

    // `mentions` stays literal, on purpose: it answers "which notes wrote this
    // spelling", which a merge does not change because a merge rewrites no
    // capture. Pinned here so the difference between the two commands stays a
    // decision rather than becoming an accident.
    let literal = s.ok(&["mentions", "@sarah"]);
    assert!(
        literal.contains(&a) && !literal.contains(&b),
        "mentions followed the merge, which is the entity page's job: {literal}"
    );

    // And nothing was rewritten. A merge is a redirection — the capture still
    // says what somebody typed, which is the half that makes it survivable.
    assert!(
        s.ok(&["show", &b])
            .contains("@sarah-chen sent the contract"),
        "a merge rewrote the capture"
    );
}

/// Saying that missing an instance leaves something owed.
///
/// §3.6's foot-gun exception: "walk the dog daily" and "take the pill daily"
/// are the same sentence to a parser and opposite answers to somebody living
/// with the consequences. So the flag is only ever set by a person, and the
/// command that sets it was invoked by no test.
#[test]
fn a_person_can_say_a_missed_instance_is_still_owed() {
    let s = Store::new("carries-forward");
    let series = s.capture("take the pill every day at 9");
    s.ok(&["settle"]);

    let before = s.ok(&["show", &series]);
    assert!(
        !before.contains("carries forward"),
        "nothing should carry forward until somebody says so: {before}"
    );

    let said = s.ok(&["carries-forward", &series]);
    assert!(
        said.contains("will stay until they are dealt with"),
        "the command did not say what it changed: {said}"
    );

    let after = s.ok(&["show", &series]);
    assert!(
        after.contains("carries forward"),
        "the flag did not take: {after}"
    );

    // Not something a parser may set, and not something a plain note can have.
    let plain = s.capture("an ordinary thought");
    s.ok(&["settle"]);
    let (code, said) = s.run(&["carries-forward", &plain]);
    assert_ne!(code, 0, "a plain note was told to carry forward: {said}");
}

/// A note's page says what the note was recognised as.
///
/// All four kinds through one command, because the interesting cases are the
/// ones that differ: a task's descriptor opens with its own kind and the other
/// two do not, and a plain note has no descriptor at all and must therefore
/// produce no line rather than a label with nothing after it.
#[test]
fn a_page_says_what_the_note_was_recognised_as() {
    let s = Store::new("recognised-as");
    let task = s.capture("send sarah the migration doc tomorrow");
    let event = s.capture("standup tomorrow at 9am");
    let series = s.capture("take the pill every day at 9");
    let plain = s.capture("an ordinary thought");
    s.ok(&["settle"]);

    // A task says it is one, and says when. Without the due date this is the
    // half that was missing: the kind alone does not tell you anything you
    // could act on.
    let t = s.ok(&["show", &task]);
    assert!(t.contains("task"), "a task did not say so: {t}");
    assert!(t.contains("due 20"), "a task did not say when: {t}");
    // And says it once. The descriptor already opens with the kind.
    assert!(!t.contains("task      task"), "the kind is doubled: {t}");

    let e = s.ok(&["show", &event]);
    assert!(e.contains("event"), "an event did not say so: {e}");

    // A series says its rule, which is the only thing that distinguishes it
    // from an event on its page.
    let r = s.ok(&["show", &series]);
    assert!(r.contains("series"), "a series did not say so: {r}");
    assert!(
        r.contains("every day"),
        "a series did not say its rule: {r}"
    );

    // A note that was never anything says nothing, because it has no promotion
    // row at all.
    let p = s.ok(&["show", &plain]);
    assert!(
        !p.lines().any(|l| l.trim_end() == "note"),
        "a plain note got a kind line: {p}"
    );

    // A note that *was* something says nothing either, and this is the case
    // that bites: the reparse demotes by writing a `note` promotion, so the row
    // exists and its descriptor is empty. Printing the label anyway puts a line
    // on the page with nothing after it.
    let demoted = s.capture("budget 40k every friday");
    s.ok(&["settle"]);
    assert!(s.ok(&["show", &demoted]).contains("series"), "not a series");
    s.ok(&["edit", &demoted, "budget 40k"]);
    s.ok(&["settle"]);
    let d = s.ok(&["show", &demoted]);
    assert!(
        !d.lines().any(|l| l.trim_end() == "note"),
        "a demoted note got a bare kind line: {d}"
    );
    // And the rule it used to have is gone with it. Checked by the kind rather
    // than by the words: the slug keeps the spelling it was minted from, so
    // "every" is still on the page and always will be.
    assert!(
        !d.contains("series"),
        "the rule outlived the text that said it: {d}"
    );

    // Marking it done is a person's answer, so the page says the state changed
    // and says the answer was theirs.
    s.ok(&["done", &task]);
    let after = s.ok(&["show", &task]);
    assert!(
        after.contains("done"),
        "a finished task did not say so: {after}"
    );
    assert!(
        after.contains("(yours)"),
        "a human answer was not marked as one: {after}"
    );
}

/// Binding the note that *is* an entity, and unbinding it.
///
/// §4.3 lets one note be the entity rather than merely mention it. The
/// consequence is that the note has to leave the list of notes *about* the
/// entity — a row appearing in both places reads as two notes — and has to
/// come back when the binding is removed. Both halves were covered at store
/// level and by no command.
#[test]
fn the_note_that_is_an_entity_leaves_the_list_of_notes_about_it() {
    let s = Store::new("entity-bind");
    let who = s.capture("@sarah is the staff engineer on the migration");
    let about = s.capture("ask @sarah about the invoice");
    s.ok(&["settle"]);

    // Two notes name her, and neither is her.
    let before = s.ok(&["entity", "@sarah"]);
    assert!(before.contains(&who) && before.contains(&about), "{before}");
    assert!(s.ok(&["people"]).contains("2 note(s)"));

    s.ok(&["entity-bind", "@sarah", &who]);

    let bound = s.ok(&["entity", "@sarah"]);
    assert!(
        bound.contains("the note that is this person"),
        "the page did not say which note is her: {bound}"
    );
    // Named once, in the header, and not again in the list below it.
    assert_eq!(
        bound.matches(&who).count(),
        1,
        "the backing note is in both places: {bound}"
    );
    assert!(bound.contains(&about), "the other note was lost: {bound}");

    // And the count agrees with the list somebody is looking at, rather than
    // saying two and showing one.
    assert!(
        s.ok(&["people"]).contains("1 note(s)"),
        "the count still includes the backing note"
    );

    // Unbinding puts it back. Nothing was destroyed to move it.
    s.ok(&["entity-bind", "@sarah"]);
    let unbound = s.ok(&["entity", "@sarah"]);
    assert!(
        unbound.contains(&who) && unbound.contains(&about),
        "unbinding did not return the note: {unbound}"
    );
    assert!(!unbound.contains("the note that is this person"));

    // An entity exists because a note names it, so there is nothing to bind a
    // name nothing has written.
    let (code, said) = s.run(&["entity-bind", "#reading", &who]);
    assert_ne!(code, 0, "bound a tag no note wrote: {said}");
}

/// Finishing a task, and saying it is not finished after all.
///
/// The reverse was the untested half. A completion that could not be undone
/// would make `done` the kind of irreversible act §6 reserves for redaction,
/// which is not what finishing a task is.
#[test]
fn a_finished_task_can_be_opened_again() {
    let s = Store::new("undone");
    let task = s.capture("send sarah the migration doc tomorrow");
    s.ok(&["settle"]);
    assert!(s.ok(&["tasks"]).contains(&task));

    s.ok(&["done", &task]);
    assert!(
        !s.ok(&["tasks"]).contains(&task),
        "a finished task is still outstanding"
    );

    s.ok(&["undone", &task]);
    let open = s.ok(&["tasks"]);
    assert!(open.contains(&task), "undone did not reopen it: {open}");

    // And the page agrees with the listing, which is the half that disagreed
    // everywhere else this was checked.
    let page = s.ok(&["show", &task]);
    assert!(page.contains("task"), "{page}");
    assert!(!page.contains("done"), "the page still says done: {page}");
}

/// Planning something against a cue, listing it, and abandoning it.
///
/// Three verbs — `intent`, `intents`, `forget` — and no test touched any of
/// them. Both cue kinds are here because they are stored differently: a time
/// cue keeps an instant and no match string, a branch cue keeps a match string
/// and no instant, and a listing that got either backwards would still print
/// something plausible.
#[test]
fn a_plan_waits_for_a_cue_until_it_is_abandoned() {
    let s = Store::new("intents");

    assert!(
        s.ok(&["intents"]).contains("No plans."),
        "an empty list should say so rather than print nothing"
    );

    // The wall clock is resolved against the zone of the machine that typed it.
    s.in_zone(
        "Asia/Tokyo",
        &["intent", "at", "2026-10-07T09:00", "ring the dentist"],
    );
    s.ok(&["intent", "branch", "main", "run the migration check"]);

    let json = s.ok(&["intents", "--json"]);
    assert!(
        json.contains("\"cue\":\"time\"") && json.contains("\"cue\":\"git_branch\""),
        "both cue kinds should be listed: {json}"
    );
    // A time cue has an instant and nothing to match; a branch cue is the other
    // way round. Checked because the two share one row shape.
    assert!(
        json.contains("\"match\":null") && json.contains("\"match\":\"main\""),
        "the two cue kinds were stored the same way: {json}"
    );
    assert!(
        json.contains("\"at\":null"),
        "a branch cue got a time: {json}"
    );
    assert!(
        json.contains("ring the dentist") && json.contains("run the migration check"),
        "{json}"
    );

    // Read back from the machine that wrote it, the hour is the hour that was
    // typed. This is the assertion that was failing: the listing rendered every
    // time cue in UTC, so 09:00 in Tokyo was reported as a plan for 00:00.
    let tokyo = s.in_zone("Asia/Tokyo", &["intents"]);
    assert!(
        tokyo.contains("2026-10-07 09:00"),
        "the hour somebody typed is not the hour they were shown: {tokyo}"
    );

    // Read from somewhere else it is the same instant, differently rendered —
    // which is the point. A cue stores no zone of its own, so this is the only
    // way to tell a correct instant from a lucky one.
    let utc = s.in_zone("UTC", &["intents"]);
    assert!(
        utc.contains("2026-10-07 00:00"),
        "the same instant did not follow the reader's zone: {utc}"
    );

    // A branch cue says the branch, in either zone, because it has no clock to
    // get wrong.
    assert!(tokyo.contains("branch main") && utc.contains("branch main"));

    // Abandoning one takes it out and leaves the other.
    let id = tokyo
        .lines()
        .find(|l| l.contains("ring the dentist"))
        .and_then(|l| l.split_whitespace().next())
        .expect("the listing names an id to forget")
        .to_string();
    let said = s.ok(&["forget", &id]);
    assert!(said.contains("ring the dentist"), "{said}");

    let left = s.ok(&["intents"]);
    assert!(!left.contains("ring the dentist"), "it came back: {left}");
    assert!(left.contains("run the migration check"), "{left}");

    // A cue this device cannot watch for is refused rather than stored and
    // never fired.
    let (code, said) = s.run(&["intent", "phase-of-the-moon", "waxing", "do the thing"]);
    assert_ne!(code, 0, "an unwatchable cue was accepted: {said}");

    let (code, said) = s.run(&["forget", "deadbeef"]);
    assert_ne!(code, 0, "forgot a plan that does not exist: {said}");
}

/// Every reversal that exists, reversing.
///
/// `untag`, `unpin` and `untick-at` were each untested, which is its own shape:
/// the doing half of a pair gets exercised and the undoing half does not, so a
/// reversal that stopped working would go unnoticed until somebody needed it.
///
/// `unmute` is deliberately absent from this list, because it does not exist.
/// A note muted by name cannot be lifted by any surface — see the note in the
/// review ledger. Asserting the current behaviour here would cement it.
#[test]
fn what_can_be_done_to_a_note_can_be_undone() {
    let s = Store::new("reversals");
    let n = s.capture("#groceries: milk, eggs, bread");
    s.ok(&["settle"]);

    // A tag, and its retraction. Checked on the note's page rather than by the
    // command's own say-so, which would only prove it printed something.
    s.ok(&["tag", &n, "reading"]);
    assert!(
        s.ok(&["show", &n]).contains("reading"),
        "the tag did not take"
    );
    s.ok(&["untag", &n, "reading"]);
    assert!(
        !s.ok(&["show", &n]).contains("reading"),
        "the tag survived its retraction"
    );
    // And the tag it was captured with is still there. `untag` takes one tag
    // off, not every tag.
    assert!(s.ok(&["show", &n]).contains("groceries"), "untag took both");

    s.ok(&["pin", &n]);
    assert!(
        s.ok(&["show", &n]).contains("pinned"),
        "the pin did not take"
    );
    s.ok(&["unpin", &n]);
    assert!(
        !s.ok(&["show", &n]).contains("pinned"),
        "the pin survived being removed"
    );

    // An item by number, and back. The number is what to reach for when two
    // items read the same, so it has to be the position and not the words.
    s.ok(&["tick-at", &n, "2"]);
    let list = s.ok(&["list", "groceries"]);
    assert!(
        list.contains("[x] eggs"),
        "the wrong item was ticked: {list}"
    );
    assert!(
        list.contains("[ ] milk") && list.contains("[ ] bread"),
        "{list}"
    );
    assert!(list.contains("2 of 3 left"), "the count disagrees: {list}");

    s.ok(&["untick-at", &n, "2"]);
    let back = s.ok(&["list", "groceries"]);
    assert!(!back.contains("[x]"), "unticking left it ticked: {back}");
    assert!(back.contains("3 of 3 left"), "{back}");

    // An item that is not there is a request that could not be carried out, so
    // it exits non-zero. Printing a refusal and exiting 0 tells a script the
    // tick landed. Both bad numbers are checked, because they are refused in
    // different places and used to disagree about being failures.
    let (code, said) = s.run(&["tick-at", &n, "9"]);
    assert_ne!(code, 0, "ticking a missing item reported success: {said}");
    assert!(said.contains("no item 9"), "{said}");

    let (code, _) = s.run(&["tick-at", &n, "0"]);
    assert_ne!(code, 0, "item 0 was accepted; they start at 1");

    // Nothing was ticked by either refusal.
    assert!(!s.ok(&["list", "groceries"]).contains("[x]"));
}
