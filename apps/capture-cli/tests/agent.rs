//! An agent through the command line, as a harness would drive it
//! (an-agent-keeps-notes-through-the-cli 4.1–4.6).
//!
//! Every write here is one a harness makes: `CAPTURE_AS` set, `--json` on, the
//! text on standard input, and the answer parsed rather than read.

use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct Store(PathBuf);

impl Store {
    fn new(name: &str) -> Store {
        let d =
            std::env::temp_dir().join(format!("capture-agent-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let s = Store(d);
        s.person(&["init"]);
        s
    }

    fn run(&self, agent: Option<&str>, args: &[&str], stdin: Option<&str>) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_capture"));
        c.args(args)
            .env("CAPTURE_HOME", &self.0)
            .env_remove("CAPTURE_AS")
            .env("TZ", "UTC")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(a) = agent {
            c.env("CAPTURE_AS", a);
        }
        let mut child = c.spawn().expect("capture runs");
        if let Some(text) = stdin {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(text.as_bytes())
                .unwrap();
        } else {
            drop(child.stdin.take());
        }
        child.wait_with_output().unwrap()
    }

    fn person(&self, args: &[&str]) -> String {
        let out = self.run(None, args, None);
        assert!(
            out.status.success(),
            "capture {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// As the agent `claude`, with `--json`, requiring success and one object.
    fn agent(&self, args: &[&str], stdin: Option<&str>) -> Value {
        let out = self.run(Some("claude"), args, stdin);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "CAPTURE_AS=claude capture {args:?}: {stdout}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
            panic!("capture {args:?} printed something that is not JSON ({e}):\n{stdout}")
        })
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_harness_writes_reads_and_is_refused_a_stale_edit() {
    let s = Store::new("loop");
    s.person(&["agent", "add", "claude"]);

    let made = s.agent(&["page", "new", "Car maintenance", "--json"], None);
    assert_eq!(made["name"], "car-maintenance", "{made}");
    assert_eq!(made["kind"], "page");
    assert_eq!(made["by"], "claude");
    let v1 = made["version"].as_str().unwrap().to_string();
    assert_eq!(v1.len(), 64);

    // Three lines through stdin, line breaks kept.
    let edited = s.agent(
        &[
            "edit",
            "car-maintenance",
            "--expect",
            &v1,
            "--stdin",
            "--json",
        ],
        Some("Car maintenance\n- [ ] tyres\n- [ ] oil\n"),
    );
    let v2 = edited["version"].as_str().unwrap().to_string();
    assert_ne!(v1, v2);

    let shown: Value =
        serde_json::from_str(&s.person(&["show", "car-maintenance", "--json"])).unwrap();
    assert_eq!(shown["version"], v2.as_str());
    assert_eq!(shown["text"], "Car maintenance\n- [ ] tyres\n- [ ] oil");
    assert_eq!(shown["by"], "claude");

    // The person saves; the agent's edit from v2 is refused, and nothing moves.
    s.person(&["edit", "car-maintenance", "Car maintenance (mine now)"]);
    let stale = s.run(
        Some("claude"),
        &[
            "edit",
            "car-maintenance",
            "--expect",
            &v2,
            "--json",
            "--",
            "overwritten",
        ],
        None,
    );
    assert_eq!(stale.status.code(), Some(4), "the conflict code");
    assert!(stale.stdout.is_empty(), "nothing on stdout for a refusal");
    let err: Value = serde_json::from_str(String::from_utf8_lossy(&stale.stderr).trim()).unwrap();
    assert_eq!(err["error"], "conflict");
    let shown: Value =
        serde_json::from_str(&s.person(&["show", "car-maintenance", "--json"])).unwrap();
    assert_eq!(shown["text"], "Car maintenance (mine now)");

    // Appending needs no version and keeps the person's edit.
    s.agent(
        &["append", "car-maintenance", "--stdin", "--json"],
        Some("- [ ] wipers\n"),
    );
    let shown: Value =
        serde_json::from_str(&s.person(&["show", "car-maintenance", "--json"])).unwrap();
    assert_eq!(shown["text"], "Car maintenance (mine now)\n- [ ] wipers");

    // Found by search; absent from the person's timeline.
    let found: Value = serde_json::from_str(&s.person(&["search", "wipers", "--json"])).unwrap();
    assert_eq!(found.as_array().map(Vec::len), Some(1), "{found}");
    let timeline = s.person(&["timeline", "--json"]);
    assert!(!timeline.contains("car-maintenance"), "{timeline}");

    // And the log names the agent: every event but the person's own edit.
    let log = s.person(&["log", "car-maintenance", "--json"]);
    let log: Value = serde_json::from_str(&log).unwrap();
    let origins: Vec<&str> = log
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["origin"].as_str().unwrap())
        .collect();
    let distinct: std::collections::BTreeSet<_> = origins.iter().collect();
    assert_eq!(distinct.len(), 2, "two writers: {origins:?}");
}

#[test]
fn an_agent_builds_linked_pages_in_any_order_by_title() {
    // The way a model writes a wiki: the page that links first, its targets
    // after, links written as the titles it sees. One command a page.
    let s = Store::new("wiki");
    s.person(&["agent", "add", "claude"]);
    let hub = s.agent(
        &["page", "new", "Local models", "--stdin", "--json"],
        Some("Start with [[KV cache maths]] and [[Qwen3.8-27B]].\n"),
    );
    assert_eq!(hub["name"], "local-models", "{hub}");
    s.agent(
        &[
            "page",
            "new",
            "KV cache maths",
            "--under",
            "local-models",
            "--stdin",
            "--json",
        ],
        Some("Back to [[Local models]].\n"),
    );
    s.agent(
        &["page", "new", "Qwen3.8-27B", "--stdin", "--json"],
        Some("A model.\n"),
    );

    let shown: Value =
        serde_json::from_str(&s.person(&["show", "local-models", "--json"])).unwrap();
    assert_eq!(
        shown["text"],
        "Local models\n\nStart with [[KV cache maths]] and [[Qwen3.8-27B]]."
    );
    let links: Value =
        serde_json::from_str(&s.person(&["links", "local-models", "--json"])).unwrap();
    for l in links["links"].as_array().unwrap() {
        assert!(l["resolved"].is_string(), "every link resolves: {links}");
    }
    let back = links["backlinks"].as_array().unwrap();
    assert_eq!(back.len(), 1, "{links}");
    assert_eq!(back[0]["name"], "kv-cache-maths");
}

#[test]
fn an_agent_is_refused_a_credential_and_the_persons_acts() {
    let s = Store::new("refused");
    s.person(&["agent", "add", "claude"]);
    s.agent(&["page", "new", "Deploy", "--json"], None);

    let out = s.run(
        Some("claude"),
        &[
            "append",
            "deploy",
            "--json",
            "--",
            "API key: sk-abc123def456ghi789jkl012",
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(5));
    let err: Value = serde_json::from_str(String::from_utf8_lossy(&out.stderr).trim()).unwrap();
    assert_eq!(err["error"], "refused");

    for args in [
        &["redact", "deploy", "--force"][..],
        &["forget", "abc"][..],
        &["retire", "claude"][..],
        &["set", "quiet.start", "22:00"][..],
        &["agent", "add", "helper"][..],
        &["rebuild"][..],
    ] {
        let out = s.run(Some("claude"), args, None);
        assert_eq!(
            out.status.code(),
            Some(5),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let shown = s.person(&["show", "deploy"]);
    assert!(!shown.contains("sk-abc"), "{shown}");
}

#[test]
fn a_retired_agent_is_listed_and_cannot_write() {
    let s = Store::new("retired");
    s.person(&["agent", "add", "claude"]);
    let listed: Value = serde_json::from_str(&s.person(&["agent", "list", "--json"])).unwrap();
    assert_eq!(listed[0]["name"], "claude");
    assert_eq!(listed[0]["retired"], false);
    assert_eq!(listed[0]["here"], true);

    s.person(&["retire", "claude", "--yes"]);
    let out = s.run(Some("claude"), &["+", "after"], None);
    assert_eq!(
        out.status.code(),
        Some(5),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let listed: Value = serde_json::from_str(&s.person(&["agent", "list", "--json"])).unwrap();
    assert_eq!(listed[0]["retired"], true);
}

#[test]
fn the_persons_writes_answer_json_too() {
    let s = Store::new("person-json");
    let out = s.person(&["--json", "+", "a thought"]);
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["kind"], "note");
    assert!(v["by"].is_null());
    let name = v["name"].as_str().unwrap().to_string();
    let tagged: Value =
        serde_json::from_str(s.person(&["tag", &name, "idea", "--json"]).trim()).unwrap();
    assert_eq!(tagged["name"], name.as_str());
    // A read still takes --json the way it always did.
    let shown: Value = serde_json::from_str(&s.person(&["--json", "show", &name])).unwrap();
    assert_eq!(shown["tags"][0], "idea");
}

#[test]
fn instructions_are_printed_and_nothing_else_is_written() {
    let s = Store::new("instructions");
    let before: Vec<_> = walk(&s.0);
    let out = s.person(&["agent", "instructions", "--name", "claude"]);
    assert!(out.contains("CAPTURE_AS=claude capture append"));
    // The settings block is the one line after its marker; a note follows it.
    let json = out.split("----\n").last().unwrap().lines().next().unwrap();
    let v: Value = serde_json::from_str(json.trim()).unwrap();
    assert!(
        out.contains("trusted"),
        "says the allowlist needs a trusted folder"
    );
    let allow: Vec<&str> = v["permissions"]["allow"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect();
    assert!(allow.contains(&"Bash(capture search:*)"));
    assert!(allow.contains(&"Bash(CAPTURE_AS=claude capture edit:*)"));
    for bad in ["redact", "forget", "retire", "pair", "set"] {
        assert!(
            !allow.iter().any(|a| a.contains(&format!("capture {bad}"))),
            "{bad}"
        );
    }
    assert_eq!(walk(&s.0), before, "the store is untouched");
}

fn walk(d: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out.sort();
    out
}
