## Why

DESIGN names a command that does not exist, twice.

§1.1, limit 5 — the section listing what redaction cannot reach:

> **Text that other notes copied is not reachable.** … §4.3 therefore forbids
> deriving slugs from sensitive-looking captures and **provides
> `redact --with-links`**, which redacts inbound linking text alongside the
> target.

§4.3, again: *"`redact --with-links` remains the other mitigation."*

The string `with-links` appears nowhere in the workspace. `capture redact` takes
`--cancel`, `--all-since` and `--force`, and nothing else.

The hole it was provided for is real and the design states it plainly. A note's
slug is derived from its opening words, and a note linking to it carries that
slug **inside its own verbatim text**. Redacting the target re-derives its slug
from its id so the name stops carrying the words — but the copies already
written into other notes are untouched, and `purge_note` only deletes
`links WHERE src = <target>`, which is the target's outbound links, not the
inbound ones.

So the words survive in the linking notes' text and in their search index. The
one mitigation the design says is provided is absent, which makes the stated
hole wider than §1.1 admits.

This is the last of the eight findings from auditing DESIGN's present-tense
claims against the code.

## What Changes

- **`capture redact --with-links <name>`** names the target and every note
  linking to it in one redaction.
- **The confirmation lists every note that will be destroyed**, by name. §1.1
  makes this the one confirmation that must not be trained away, and a flag
  that quietly widens the blast radius is exactly what would train it away.
- **It refuses rather than truncating** when the target and its backlinks
  exceed the cap a redaction may name. Silently redacting the first sixty-four
  would leave the rest holding the words while reporting success.
- **Without the flag, nothing changes.** `capture redact` still names one note.

## Capabilities

### Modified Capabilities

- `cli`: that a redaction can take the notes linking to its target with it.

## Impact

- `crates/capture-store/src/session.rs` — a redaction naming several notes.
- `apps/capture-cli/src/{args,write}.rs` — the flag and the confirmation.

## Non-goals

- **Editing the linking notes' text.** §3.1.3 keeps captured text verbatim, and
  rewriting somebody's note to remove a word somebody else chose is a worse
  answer than destroying it on request. The design's own wording — redacting
  the inbound linking text — is a redaction, and a redaction destroys.
- **Doing it by default.** The flag exists because the trade is the user's: a
  linking note may be entirely innocuous and valuable, and taking it is a
  decision only they can make.
- **Following links transitively.** One hop. A note linking to a note linking
  to the target has copied nothing of it, and a transitive sweep is how a
  deliberate act becomes an accident.
- **Reaching links in notes this device does not hold.** It cannot, and §1.1
  already says so.
