## Why

Redaction destroys a note's text and leaves the context it was captured in.

`purge_note` clears the search index, the link rows, the mentions, the list
items and the text itself. It does not touch `ambient`, which holds the
foreground **application**, the **window title**, the **working directory**, the
**repository** and the **branch** that were recorded when the note was made.

So after destruction `state.db` still answers "which repo was this written in",
and `by_context` — which joins `notes` to `ambient` with no redaction filter —
still returns the destroyed note for a context query, with that context attached.
It clears only on a full rebuild, which may never happen.

§1.1 makes the argument itself, about a different table:

> **Redaction purges the projection too.** Destroying `body` in `events.db`
> while leaving the search index intact would leave the redacted password fully
> searchable on every replica — a complete failure of the stated purpose.

`purge_note`'s own comment records finding exactly this class of bug twice
before, for `mentions` and `list_items`: *"a purge is not a fold — it edits the
projection in place … what survived was the fact of the mention, which is the
part §1.1 is about."* `ambient` was not included either time.

A window title is often the same material as the note. "Reset password —
Fastmail" beside a destroyed note about a password is the failure §1.1 describes,
in a column instead of an index.

Found by auditing DESIGN's claims against the code.

## What Changes

- **Redaction deletes the note's ambient row**, with the other derived rows it
  already deletes.
- **A destroyed note stops appearing in context queries**, because the row it
  was matched by is gone.
- **The stored window title goes with it.** It is written at capture and read by
  nothing — `ambient_of` selects app, cwd, repo and branch — so today it is
  material that is kept, never used, and never purged.

## Capabilities

### Modified Capabilities

- `projection`: that purging a note removes the context it was captured in.

## Impact

- `crates/capture-store/src/projection.rs` — one delete in `purge_note`.

## Non-goals

- **Not collecting ambient context.** §2.3 argues for it and this does not
  revisit that; what is captured alongside a note is useful while the note
  exists.
- **Removing the unread `title` column.** It is written and never read, which is
  worth knowing separately — but a column that is now purged with its note is no
  longer a leak, and deleting schema is a different change.
- **Redaction of context on other replicas.** This is the projection, which
  every replica purges for itself when it carries the redaction out.
