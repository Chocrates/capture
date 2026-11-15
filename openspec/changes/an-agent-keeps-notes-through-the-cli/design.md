## Context

Every event is signed by an origin `dev = H(pubkey ‖ incarnation)`; the roster
records origins and their role (`full | leaf`). One process holds the writer
lock (normally the overlay); the CLI opens read-only and submits writes to the
holder, which signs with the device's identity. There is no per-event author
beyond the origin. The timeline is ordered by capture time, so edits do not
reorder it, but every capture appears in it and in review.

## Goals / Non-Goals

**Goals:** an agent's writes are attributable, revocable, kept out of the
person's queues, and safe against overwriting the person; a harness needs
nothing beyond `capture` and its JSON.

**Non-Goals:** MCP, read restrictions, phone agents, enrichment.

## Decisions

### D1. An agent is an origin, not a flag

`capture agent add NAME` generates a key pair and incarnation, stores them at
`<store>/config/agents/NAME.key` (0600, like `identity.key`), and has the
device's own origin write a roster event enrolling it with role `agent`, bound
to the enrolling device.

A write submitted with `CAPTURE_AS=NAME` is signed by the lock holder with that
agent's key. Agent origins have their own `seq` and chain, so they sync,
verify and retire exactly as devices do.

Rejected:
- **A trailing `by` field on events.** It would be a self-assertion signed by
  the device key — any process could set or omit it. An origin is a key; it
  cannot be faked by a writer that does not hold it, and it can be retired.
- **A `#agent` tag convention.** Removable by any edit; carries no authority.
- **The agent as a separately paired device.** Pairing needs a second sync
  endpoint and a SPAKE2 ceremony for something living on the same disk.

### D2. The role is read from the roster as it stood when the event was written

The fold already takes only the event set; it learns an origin is an agent from
the enrolling roster event in the log. A note is *agent-originated* when its
root capture's origin has role `agent`.

Agent-originated notes are excluded from the timeline, review, settle and
"recent" surfaces, and are included in search, pages, links, entity pages and
`show`, labelled "written by NAME". An agent's edit to the person's note is
still shown on that note, attributed in its history; it does not move the note.

This keeps §3.6: an agent writing fifty pages creates zero items the person is
asked to look at.

### D3. Credential-shaped agent text is refused

For agent-origin writes the store runs `looks_like_credential` on the text
before signing and refuses with `ExitCode::Refused` and a reason. For human
writes it keeps flagging (D3 of `pages-that-hold-up`). An agent has no reason
to store a secret, and refusing is cheaper than redacting later.

### D4. Write commands speak JSON

Under `--json`, every write prints one object: `{"name", "slug", "version",
"kind"}` and any `notice` (name taken, flagged). Errors under `--json` print
`{"error": code, "message"}` on stderr with the typed exit code. The view
shape is built once and rendered as prose or JSON, as reads already are
(json.rs).

### D5. `append` is safe without `--expect`

`capture append NAME` (text from args or stdin) adds lines to the end of the
note's current text inside the writer lock, writing an edit whose `prev` is
the head at that moment. Nothing written concurrently is lost because the new
version contains it. `edit` still replaces the whole text and is where
`--expect` matters.

### D6. The skill and allowlist are printed

`capture agent instructions [--name NAME]` prints:
- a Claude Code skill (`SKILL.md`): search → `show --json` → `edit --expect`
  or `append`; on conflict, re-read and retry once, then tell the person; never
  `redact`;
- a `.claude/settings.json` permission block allowing `capture search|show|
  links|log|timeline|agenda|cal|tasks|entity|people|projects ...` and
  `CAPTURE_AS=NAME capture page|edit|append|tag|alias|parent ...`.

Printed, never written — the same reason `install` prints the shell snippet
(README: "a program that makes them silently is one you cannot tell to stop").

## Risks / Trade-offs

- [New roster role is a CBOR enum variant; an older build holds the roster
  event as malformed] → For a roster event that is worse than for a promotion:
  an older peer would not know the agent origin and would hold every agent
  event. Both devices update together (as with `Promoted::Page`); the phone is
  updated first, and `capture agent add` refuses until every roster device has
  been seen on a build that knows the role (peer build version in sync state).
- [An env var is easy to forget; the agent then writes as the person] → The
  printed allowlist only admits writes prefixed with `CAPTURE_AS=NAME`; bare
  write commands need approval.
- [Whole-archive read sends notes to the model provider] → The person's choice;
  `agent instructions` says it in its first line.
- [Agent key on disk lets any local process write as the agent] → Same trust
  as `identity.key`; retiring the agent invalidates its future events.

## Open Questions

- Should an agent be able to write captures at all, or only pages? Default:
  both, with captures also kept out of the timeline (D2).
