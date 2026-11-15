Depends on `pages-that-hold-up` tasks 1.2 (search quoting) and 1.5 (expected version).

## 1. Agent origin in the log

- [x] 1.1 Tests first: a roster event with role `agent` round-trips (golden bytes pinned); an older-decoder test shows it is held as malformed, not dropped.
- [x] 1.2 `capture-core`: roster role `agent`; fold derives "agent-originated" from the root capture's origin role as the roster stood (**property**: fold result independent of event delivery order).
- [x] 1.3 `capture-store`: `agents::add(name)` writes `config/agents/NAME.key` (0600) and the enrolling roster event; `agents::list`; retire reuses the device path.
- [x] 1.4 Writer lock: a submission names its origin; the holder signs with that agent's key; unknown or retired agent → refused.
- [x] 1.5 ~~Peer-build gate~~ not needed: the role is a trailing field an older build reads past (see design Risks).

  - 2026-11-14: role and name are fields 4 and 5 of `OriginClaimBody`, golden vector pinned, and a decoder shaped like the old body reads the new bytes. `Roster::agents`, `Roster::agent(dev)`, `Roster::devices()`. Agents cannot admit, retire, re-key, re-admit or co-sign; the root cannot be one; order-independence tested over all permutations. "Agent-originated" is derived in the projection by joining `note_origins` to `agents`, not in the core fold (the fold has no roster) — the property it needed (independent of delivery order) holds because the roster is. Key at `config/agents/NAME.key` (0600, tested), a `NAME.seq` watermark refuses a write after the log went backwards (tested).

## 2. Keeping agent notes out of the way

- [x] 2.1 Tests: agent-originated notes absent from timeline, review, settle, recent; present in search, pages, links, entity pages, with the agent's name.
- [x] 2.2 Projection: mark agent-originated notes; filter the four surfaces; label in `show`/search JSON and prose.
- [x] 2.3 Desktop and phone draw the "written by NAME" label (read-only; agents only write from the CLI).

  - Timeline, review (all three tiers) and the reading queue filter them; search, `show`, links and entity pages do not, and carry `by`. The sweep does not read an agent's capture: "call the dentist tomorrow at 9" written by an agent becomes no task and rings nothing. Desktop draws "written by NAME" on every note row; the phone on the note screen (FFI `NoteSummary.by`, app built; not run on the emulator).

## 3. Credential refusal

- [x] 3.1 Test then implement: agent-origin text that looks like a credential is refused before signing; human text is still only flagged.

  - Checked on capture and edit bodies before signing (so `append`, `edit`, `page new`, `+`), `ExitCode::Refused = 5`. The person's text is still only flagged (tested side by side).

## 4. CLI

- [x] 4.1 `--json` on every write command (one view, two renderings, as json.rs does for reads); JSON errors on stderr.
- [x] 4.2 `edit --stdin`; `edit --expect VERSION` wired to the store's expected-version edit.
- [x] 4.3 `capture append NAME [TEXT | --stdin]` under the writer lock (**property**: concurrent appends and edits never lose a version's content).
- [x] 4.4 `capture agent add|list|instructions`; `CAPTURE_AS`; `capture help` lists them.
- [x] 4.5 The printed skill and allowlist; a test parses the allowlist and asserts it excludes `redact`, `forget`, `retire`, `pair`, `set`.
- [x] 4.6 `make smoke` gains an agent pass: add agent, write a page as it, read it back with JSON, stale expect refused.

  - `--json` on writes is one switch in `main.rs`: prose goes to stderr, stdout gets `{name, slug, version, kind, by, notice}`; errors `{error, message}` on stderr with the typed code. Write verbs take it anywhere before `--`; `+` only as `capture --json + ...`, since every word after `+` is content. CLI `edit` now goes through `mutate::edit_expecting` (it had its own copy). `--expect` takes the full id `show --json` prints. `append` is `mutate::append`; property-tested (24 cases) that an append's result starts with the text it followed. `capture retire NAME` finds an agent by name. The skill and allowlist are printed by `capture agent instructions`; two unit tests and a CLI test check the allowlist (reads bare, writes only behind `CAPTURE_AS=NAME`, no redact/forget/retire/pair/set) and that nothing is written. `make smoke` gains the agent pass; its steps were run by hand against a scratch store (the harness would not run `make smoke` itself, which removes its scratch directory).

## 5. Verification

- [x] 5.1 `make ci` green; sabotage each new test once.
  - Sabotaged, each caught: timeline filter, review filter, reading-queue filter, credential refusal, spool guard, per-origin watermark, local enrolment refreshing `agents`, rebuild writing `agents`, roster refusing an agent's admission, CLI allowlist, append following the head, agent attached after the opening work.
- [x] 5.2 Against a scratch `CAPTURE_HOME` (never the real archive), run Claude Code with the printed skill and allowlist: store a fact, retrieve it in a fresh session, edit after the person edited, see the conflict handled.
  - 2026-11-15, headless `claude -p` (Opus 5.5) in a scratch project holding the printed SKILL.md and settings.json, scratch store only. Stored a printer fact; a fresh session found it by search and said the agent wrote it. Built a three-page wiki children-first with `[[Title]]` links, filed them with `parent`; every link resolved. Resumed with a version read before the person appended a line: exit 4, re-read, redid the edit, the person's line kept. Nothing reached the timeline.
  - Found and fixed: (1) every command refused, reads too — Claude Code ignores a project's allow rules until the folder is trusted, and said so only on stderr; the printed instructions now say it. (2) Every page had its title twice: "a page's first line is its title" was read as "write it"; the skill now says stdin is the body only, and `page new --stdin` drops a first line that repeats the title. (3) The skill did not mention `capture parent`, so the agent asked how to file a page under one made later; it does now.
  - Seen, not changed: a compound command with an un-allowlisted part (`; echo "exit=$?"`, a `for` loop) is refused as a whole; the agent split it and went on.
- [ ] 5.3 Real use: give the real archive an agent for a week; note what it wrote that you would not have, and whether anything leaked into the timeline.
