Depends on `pages-that-hold-up` tasks 1.2 (search quoting) and 1.5 (expected version).

## 1. Agent origin in the log

- [ ] 1.1 Tests first: a roster event with role `agent` round-trips (golden bytes pinned); an older-decoder test shows it is held as malformed, not dropped.
- [ ] 1.2 `capture-core`: roster role `agent`; fold derives "agent-originated" from the root capture's origin role as the roster stood (**property**: fold result independent of event delivery order).
- [ ] 1.3 `capture-store`: `agents::add(name)` writes `config/agents/NAME.key` (0600) and the enrolling roster event; `agents::list`; retire reuses the device path.
- [ ] 1.4 Writer lock: a submission names its origin; the holder signs with that agent's key; unknown or retired agent → refused.
- [ ] 1.5 Peer-build gate: `agent add` refuses until every roster device has been seen on a build that knows the role.

## 2. Keeping agent notes out of the way

- [ ] 2.1 Tests: agent-originated notes absent from timeline, review, settle, recent; present in search, pages, links, entity pages, with the agent's name.
- [ ] 2.2 Projection: mark agent-originated notes; filter the four surfaces; label in `show`/search JSON and prose.
- [ ] 2.3 Desktop and phone draw the "written by NAME" label (read-only; agents only write from the CLI).

## 3. Credential refusal

- [ ] 3.1 Test then implement: agent-origin text that looks like a credential is refused before signing; human text is still only flagged.

## 4. CLI

- [ ] 4.1 `--json` on every write command (one view, two renderings, as json.rs does for reads); JSON errors on stderr.
- [ ] 4.2 `edit --stdin`; `edit --expect VERSION` wired to the store's expected-version edit.
- [ ] 4.3 `capture append NAME [TEXT | --stdin]` under the writer lock (**property**: concurrent appends and edits never lose a version's content).
- [ ] 4.4 `capture agent add|list|instructions`; `CAPTURE_AS`; `capture help` lists them.
- [ ] 4.5 The printed skill and allowlist; a test parses the allowlist and asserts it excludes `redact`, `forget`, `retire`, `pair`, `set`.
- [ ] 4.6 `make smoke` gains an agent pass: add agent, write a page as it, read it back with JSON, stale expect refused.

## 5. Verification

- [ ] 5.1 `make ci` green; sabotage each new test once.
- [ ] 5.2 Against a scratch `CAPTURE_HOME` (never the real archive), run Claude Code with the printed skill and allowlist: store a fact, retrieve it in a fresh session, edit after the person edited, see the conflict handled.
- [ ] 5.3 Real use: give the real archive an agent for a week; note what it wrote that you would not have, and whether anything leaked into the timeline.
