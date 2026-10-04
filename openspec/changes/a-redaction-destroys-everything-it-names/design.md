## Context

`redact_queue` is keyed by `redact_evt` alone. The fold writes a row per pending
note, so every target of one redaction overwrites the last.

The same pre-pass truncates past the cap with `take` and skips an undecodable
body with `continue`, both without a record.

## Goals / Non-Goals

**Goals.** A redaction queues every target it names. Over the cap applies to
nothing. Unreadable is visible.

**Non-Goals.** Emitting multi-target redactions. Changing the cap.

## Decisions

### `PRIMARY KEY (redact_evt, tgt)`

The pair is what a queued destruction actually is. It is a schema change to an
existing table, which `CREATE TABLE IF NOT EXISTS` will not apply — so
`PROJECTION_SCHEMA` goes to 11 and every projection is rebuilt into a fresh
file that gets the new definition.

Nothing is lost by that. The queue is derived from the log, the log has always
held every target, and the rebuild therefore queues the ones that were being
dropped. A redaction that destroyed one of three before this will destroy the
other two after it, which is what it was always ordered to do.

*Rejected:* keying by target alone. Two redactions may name the same note —
retained redactions are expected to overlap — and the queue has to be able to
say which one is being carried out.

### Over the cap is held, not truncated

`Held::Malformed` already means "this build will not act on this event and
somebody can see that". A redaction naming 100 targets is not a valid
64-target redaction; treating it as one is a decision nobody made.

*Rejected:* applying the first 64 and holding the rest. There is no "the rest"
to hold — the cap is a property of the whole event, and an event that breaks it
is malformed as a unit.

### The undecodable body too

It was `continue`, which is indistinguishable from the redaction not existing.
An authentic event this build cannot read is exactly what `held` is for, and
`capture held` now lists them.

## Risks / Trade-offs

- **A rebuild for a key change.** → §1.3 makes rebuild-not-migrate the strategy
  and the projection is disposable. This is the eleventh.
- **Archives with a half-applied multi-target redaction will finish applying
  it.** → Correct, and worth stating plainly: the remainder gets destroyed, as
  ordered. No such redaction can exist from this build, so in practice this
  affects nobody.
- **Over-cap redactions from a peer now apply to nothing where they used to
  apply to 64.** → The safe direction. Nothing is destroyed that was not
  clearly ordered, and the event is visible rather than silently partial.

## Migration Plan

`PROJECTION_SCHEMA` 10 → 11 with its changelog line. Rebuilt on next open.

## Open Questions

- **Should a held redaction be reported differently from a held capture?**
  `capture held` lists both by kind, which may be enough. A destruction order
  this build refuses to read is arguably worth more than a line in a list.
