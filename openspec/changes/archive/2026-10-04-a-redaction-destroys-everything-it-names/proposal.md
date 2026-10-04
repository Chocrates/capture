## Why

A redaction naming several notes destroys exactly one of them, and reports
success.

```sql
CREATE TABLE IF NOT EXISTS redact_queue (
  redact_evt    BLOB PRIMARY KEY,
  ...
```

The fold writes one row per pending note, and every target of one redaction
carries the same `redact_evt`. So the primary key collides and `INSERT OR
REPLACE` keeps only the last. The drain sees one row, purges one note, and the
rest keep their bodies in `events.db` and their text in the projection — for
ever, because nothing will ever queue them again.

Measured: three captures each containing "hunter2 password", one redaction
naming all three. One queue row. Two notes still readable afterwards. A
hundred-target redaction likewise yields one row.

**This is latent today.** The only shipping site that builds a `RedactBody` is
`session.rs:683`, and it always sets exactly one target; the command line, the
desktop and the binding all route through it. So no redaction issued by this
build can hit it.

It stops being latent the moment anything emits the shape DESIGN already
specifies — §1.1 is explicit that *"a redact names at most 64 targets"*, and a
per-entity or date-range redaction, a newer peer, or a future surface is the
obvious way there. At that point 63 of 64 notes ordered destroyed keep their
full text while the operation says it worked, which is being confidently wrong
in the one operation that cannot be retried.

Two related silences in the same pre-pass:

- **More than the cap is truncated, not refused.** `take(REDACT_TARGET_CAP)`
  drops targets 65 and beyond with no record anywhere. The spec says such a
  redaction *"is refused"*.
- **A redaction whose body will not decode is skipped** with `continue` and no
  `held` entry, so an unreadable destruction order is indistinguishable from
  none.

Found by auditing DESIGN's claims against the code; confirmed with a probe.

## What Changes

- **The queue is keyed by the redaction *and* the target**, so a redaction
  naming many queues many.
- **A redaction naming more than the cap is held as malformed and applies to
  nothing**, rather than silently becoming a redaction of the first 64.
- **A redaction whose body cannot be decoded is held too**, for the same
  reason: a destruction order nobody can read is a thing to see.

## Capabilities

### Modified Capabilities

- `projection`: that every target of a redaction is queued.
- `event-log`: that a redaction naming more than the cap is refused rather than
  reduced.

## Impact

- `crates/capture-store/src/projection.rs` — the key, and
  `PROJECTION_SCHEMA` 10 → 11.
- `crates/capture-core/src/fold.rs` — the cap and the undecodable body.

## Non-goals

- **Emitting multi-target redactions.** Nothing here starts naming more than
  one; this makes the mechanism correct for when something does. Building the
  surface that wants it — redact an entity, redact a day — is its own change
  with its own confirmation design.
- **Changing the cap.** 64 is §1.1's.
- **Reaching redactions already applied.** A redaction that queued one of three
  targets will queue all three after the rebuild this bumps into, because the
  queue is derived from the log and the log has always held every target.
