## Why

Asked for on 2026-11-13: an agent harness (Claude Code first) should be able to
store and retrieve information in the archive. The person chose **the whole
archive** as the agent's read scope, and chose the existing CLI over a new MCP
server: a harness with a shell can already run `capture`, and nearly every
missing piece is a CLI gap that a person writing a script hits too.

Today an agent could run `capture`, but:

- **Its writes would look like the person's.** The only author is the device
  origin. An agent's `edit` is a human edit by DESIGN.md §4's structural
  definition (~2310), pinned, and indistinguishable in `capture log`.
- **Write commands print prose.** `page new` prints `[[gym-routine]]`; a caller
  parses it to learn the name.
- **`edit` takes text only as arguments.** A multi-line page goes in as one
  quoted argument.
- **An edit can overwrite the person's newer version without knowing.** Fixed in
  the store by `pages-that-hold-up` D4; the CLI needs to expose it.
- **Nothing stops a busy agent from flooding the timeline and review** with
  captures the person never made — the manufactured backlog §3.6 forbids.

## What Changes

- **An agent writes as its own origin.** `capture agent add NAME` makes a
  separate signing identity on this device and enrols it in the roster with a
  new role `agent`. Commands run with `CAPTURE_AS=NAME` sign as it. `capture
  log` names it; `capture retire NAME` cuts it off like a lost device.
- **What an agent writes stays out of the person's way.** Notes an agent
  originated are not in the timeline, the review or the settle queue. They are
  in search, in pages, in links, and say "written by NAME".
- **An agent cannot write a credential.** Text from an agent origin that looks
  like a credential is refused, not flagged.
- **CLI for programs:** `--json` on every write command; `edit --stdin`;
  `edit --expect VERSION`; `capture append NAME` (adds lines to a page under
  the writer lock, so it never loses a concurrent edit); errors as JSON on
  stderr under `--json`.
- **A skill and an allowlist, printed, not installed.** `capture agent
  instructions` prints a Claude Code skill (the read → expect → write loop) and
  a permission allowlist that admits read commands and the agent's writes and
  leaves out `redact`, `forget`, `retire`, `pair`, `set`. The person puts them
  where they want, as with `shell-init`.

## Capabilities

### New Capabilities

- `agent-access`: agent origins, what an agent's writes do and do not touch,
  the credential refusal, the printed skill and allowlist.

### Modified Capabilities

- `cli`: JSON output from writes, `--stdin` and `--expect` on `edit`, `append`,
  JSON errors.

## Impact

- `capture-core`: roster role `agent` (a new enum variant; an older build holds
  the roster event as malformed until updated — see design Risks).
- `capture-store`: identity files per agent under `config/agents/`; the writer
  lock accepts a submission naming its origin; fold marks agent-originated
  notes; credential refusal on agent writes.
- `apps/capture-cli`: `agent add|list|instructions`, `CAPTURE_AS`, `append`,
  write-command JSON, `edit --stdin|--expect`.
- Depends on `pages-that-hold-up` (D2 search quoting, D4 expected version).

Derives from DESIGN.md §1.3 (schema evolution), §3.6 (nothing accumulates),
§4 human-edit definition (~2310), §5.5 roster and roles (~3062), the
credential rule (~2433).

## Non-goals

- An MCP server. Rejected on 2026-11-13; a thin one can wrap the same store
  functions later if a shell-less harness is wanted.
- Limiting what an agent can read. The person chose the whole archive; the
  allowlist fences writes, not reads.
- The `enrich` kind (DESIGN.md §2) — machine enrichment of the person's notes
  is a different feature.
- Agents on the phone. The CLI is desktop-only.
