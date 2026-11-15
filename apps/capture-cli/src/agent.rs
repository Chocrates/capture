//! `capture agent`: enrol a program, list them, and print what a harness needs
//! (an-agent-keeps-notes-through-the-cli D1, D6).
//!
//! # Printed, never installed
//!
//! `instructions` writes nothing outside its own output, for the reason
//! `shell-init` does not edit `~/.zshrc`: a program that sets up its own
//! permissions silently is one you cannot tell to stop. The person reads what
//! it would allow and puts it where they choose.

use capture_store::{Result, StorePaths, agents};

use crate::json::{Json, View, answer, arr, obj};

/// Enrol `name` on this device.
pub fn add(paths: &StorePaths, name: &str) -> Result<()> {
    let made = agents::add(paths, name)?;
    say!("{} is enrolled as an agent on this device.", made.name);
    say!();
    say!(
        "Write as it with:  CAPTURE_AS={} capture page new \"Build notes\"",
        made.name
    );
    say!("What it writes stays out of your timeline and review, and says who wrote it.");
    say!("Stop it at any time:  capture retire {}", made.name);
    say!();
    say!(
        "For a harness:  capture agent instructions --name {}",
        made.name
    );
    Ok(())
}

struct Agents(Vec<agents::Agent>);

impl View for Agents {
    fn json(&self) -> Json {
        arr(self.0.iter().map(|a| {
            let agents::Agent {
                name,
                pubkey,
                retired,
                here,
            } = a;
            obj([
                ("name", name.into()),
                ("key", hex(&pubkey[..8]).into()),
                ("retired", (*retired).into()),
                ("here", (*here).into()),
            ])
        }))
    }

    fn prose(&self) {
        if self.0.is_empty() {
            say!("No agents. Enrol one with:  capture agent add <name>");
            return;
        }
        for a in &self.0 {
            let state = if a.retired {
                "retired — cannot write"
            } else if a.here {
                "can write from this device"
            } else {
                "enrolled on another device"
            };
            say!("{:<16} {}  {state}", a.name, hex(&a.pubkey[..8]));
        }
    }
}

/// Every agent the roster knows.
pub fn list(paths: &StorePaths, json: bool) -> Result<()> {
    answer(&Agents(agents::list(paths)?), json);
    Ok(())
}

/// Print the skill and the allowlist for a harness.
pub fn instructions(name: &str) -> Result<()> {
    agents::check_name(name)?;
    emit!("{}", skill(name));
    say!();
    say!("---- .claude/settings.json (merge into \"permissions\") ----");
    say!("{}", allowlist(name).render());
    Ok(())
}

/// The commands an agent may run without asking, read-only ones bare and
/// writes only behind `CAPTURE_AS`, so a write that forgot the prefix — and
/// would sign as the person — needs the person's approval.
///
/// `redact`, `forget`, `retire`, `pair`, `set` and the rest of the person's
/// own acts are left out on purpose; the CLI also refuses them as an agent.
pub fn allowed(name: &str) -> Vec<String> {
    const READS: &[&str] = &[
        "search",
        "show",
        "links",
        "log",
        "timeline",
        "agenda",
        "cal",
        "tasks",
        "list",
        "mentions",
        "entity",
        "people",
        "projects",
        "agent list",
    ];
    const WRITES: &[&str] = &[
        "+", "page", "edit", "append", "tag", "untag", "alias", "parent",
    ];
    let mut out: Vec<String> = READS
        .iter()
        .map(|c| format!("Bash(capture {c}:*)"))
        .collect();
    out.extend(
        WRITES
            .iter()
            .map(|c| format!("Bash(CAPTURE_AS={name} capture {c}:*)")),
    );
    out
}

fn allowlist(name: &str) -> Json {
    obj([(
        "permissions",
        obj([("allow", arr(allowed(name).into_iter().map(Json::from)))]),
    )])
}

fn skill(name: &str) -> String {
    format!(
        r#"Everything this agent reads from the archive is sent to the model provider.
The person chose that; keep it in mind before pasting anything onward.

---- SKILL.md (e.g. ~/.claude/skills/capture-notes/SKILL.md) ----
---
name: capture-notes
description: Store and retrieve the person's notes with the `capture` CLI. Use to remember something for later, to look up what the person has written, or to keep a running page of findings.
---

# Notes through `capture`

The archive is the person's. You write into it as the agent `{name}`, and
everything you write says so. Your notes do not appear in their timeline or
review; they are found by search and by name.

## Read

    capture search <words> --json        notes matching every word, best first
    capture show <name> --json           one note; "version" is its current version
    capture links <name> --json          what it links to and what links to it

## Write — always as {name}

    CAPTURE_AS={name} capture page new "<title>" [--under <name>] --stdin --json
                                                             a page with its body; "name" is what to call it
    CAPTURE_AS={name} capture append <name> --stdin --json   add lines at the end; loses nothing
    CAPTURE_AS={name} capture edit <name> --expect <version> --stdin --json
                                                             replace the whole text
    CAPTURE_AS={name} capture tag <name> <tag> --json

## Pages and links

A page's first line is its title; give the body on stdin. Link with
`[[Title as written]]` or `[[its-name]]`, either works. A link to a page that
does not exist yet is fine: it starts working when that page is made, so
write pages in any order. Put a page under another with `--under <name>`.
Plain text only: no markdown headings, bold or tables, and a line starting
`- ` becomes a checklist item, so use `•` for an ordinary bullet.

## Changing what is there

Prefer `append`. To rewrite, read with `show --json`, then `edit --expect` with
the `version` you read. Exit code 4 means the person changed it since: read it
again, redo the edit on what is there now, and try once more. If it conflicts a
second time, stop and tell the person.

## Never

- Store a password, token or key. It is refused (exit code 5); do not try to
  get around it.
- Redact, forget, retire, pair or change settings. Those are the person's.
- Write without `CAPTURE_AS={name}`: that signs as the person.

## Errors

With `--json`, a failed command prints `{{"error": "...", "message": "..."}}` on
stderr. Codes: 1 usage, 2 not found, 3 damaged archive (stop), 4 conflict,
5 refused.
"#
    )
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_allowlist_admits_reads_and_the_agents_writes_and_nothing_destructive() {
        let rendered = allowlist("claude").render();
        let allow = allowed("claude");
        assert!(allow.contains(&"Bash(capture search:*)".to_string()));
        assert!(allow.contains(&"Bash(CAPTURE_AS=claude capture edit:*)".to_string()));
        for verb in [
            "redact", "forget", "retire", "pair", "set", "unflag", "restore", "rebuild",
        ] {
            assert!(
                !allow.iter().any(|a| a.contains(&format!("capture {verb}"))),
                "{verb} is allowed: {rendered}"
            );
        }
        // A write never bare: without the prefix it would sign as the person.
        for verb in ["edit", "append", "page", "tag", "+"] {
            assert!(
                !allow.contains(&format!("Bash(capture {verb}:*)")),
                "bare {verb} is allowed"
            );
        }
        assert!(
            rendered.starts_with(r#"{"permissions":{"allow":["#),
            "{rendered}"
        );
    }

    #[test]
    fn the_skill_names_the_agent_and_the_conflict_loop() {
        let s = skill("helper");
        assert!(s.contains("CAPTURE_AS=helper capture append"));
        assert!(s.contains("--expect"));
        assert!(s.contains("Exit code 4"));
        assert!(s.lines().next().unwrap().contains("model provider"));
    }
}
