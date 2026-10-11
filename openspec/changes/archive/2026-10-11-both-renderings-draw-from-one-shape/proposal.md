## Why

Every read command renders its answer twice, once as prose and once as
`--json`, and the two are written separately. So a field the prose shows can be
absent from the JSON, and four times now it has been: `tasks` without `band` or
`due`, `agenda` without `unread`, `agenda` without an occurrence's own `title`,
`location` and `notes`, and `projects` very nearly. Each was fixed where it was
found, and each fix left the mechanism that produced it in place.

An audit of all twenty-five `--json` commands on 2026-10-10 found it is still
happening:

- `agenda --json` carries none of the overdue tasks or owed occurrences the
  prose prints under "Still open" and "Still owed", and no `cancelled` flag for
  a cancelled-and-moved occurrence.
- `due --json` carries no delivery state. Whether an alert was delivered, is
  waiting, or is being held to morning is the entire reason the command exists
  ("why did nothing happen"), and a script asking it gets the list without the
  answer. It also says nothing about unread captures or a settle that failed.
- `show --json` carries nothing of what the note was recognised as, nor its
  links, nor when a mute ends.
- Every note in `timeline`, `search`, `during` and `show` reports
  `"muted": true` for a mute that has **ended** — `muted_until.is_some()` rather
  than `is_muted(now)`, the same mistake the unmute change fixed in ten SQL
  predicates. And a note queued for redaction is indistinguishable in JSON from
  one with no readable text.
- `tasks --json` has no text; `review --json` has no suggestion and no lapsed
  series prompt; `peers --json` has no sync state; `export --json` does not
  report the events withheld by redaction or which notes were stripped.
- `quarantine` and `held` interpolate `reason` unescaped.

## What Changes

- **Each read command builds one value, its *view*, and both renderers draw
  from it.** The prose renderer takes the view and nothing else — no store
  handle — so it cannot show a fact the view does not hold. The JSON renderer
  destructures the view exhaustively, so a field added to the view does not
  compile until the JSON names it. Together: anything in the prose is in the
  JSON, by construction rather than by review.
- **A small JSON value type** (`apps/capture-cli/src/json.rs`) replaces the
  `format!` strings, so escaping is decided once. Field order is preserved, so
  `slug` stays first in a note, which `tools/converge.sh` depends on.
- **The drift above is closed**, by adding fields. Three shapes change from a
  bare array to an object, for the reason `tasks` and `agenda` already did:
  `due` (to carry `unread` and a failed settle), `peers` (to carry the sync
  state, which is about the whole roster rather than any one row) and `review`
  (to carry the lapsed-series prompt, which is about the session).
- **`recover --json` carries each copy's first line**, as the listing prints
  it and no more.
- **`muted` means in force.** `muted_until` is added beside it, so a consumer
  can see a mute's end as well as whether it holds now.
- **A test parses every `--json` command's output** with a real JSON parser and
  checks the fields this change adds.
- **`make ci` runs `scenarios` and the new `json` suite.** Found on the way:
  the 41 scenario tests that pin nearly every fix since M1 were never part of
  the gate.

## Capabilities

### Modified Capabilities

- `cli`: the requirement that every read takes `--json` gains the guarantee
  that the JSON carries everything the prose says.

## Impact

- `apps/capture-cli/src/{read,ops,verify,backup,write}.rs` — every `--json`
  path; new `json.rs`.
- `apps/capture-cli/Cargo.toml` — `serde_json` as a **dev**-dependency only, to
  parse output in tests. Already in `Cargo.lock` through the desktop.
- `due --json`, `peers --json` and `review --json` change shape. Nothing in the
  repository consumes any of them; the two scripts that read CLI JSON read
  `timeline`.
- No store, fold, or event change.
