## Context

`ratelimit::check(history, targets, now)` is a correct pure function with good
tests. `Store::redaction_history` feeds it and is wrong four ways: it selects
from a table redactions never reach, it is not per origin, it counts events
rather than targets, and it has no test.

## Goals / Non-Goals

**Goals.** The ceiling actually bounds. Per origin, in targets, against the
redaction's own time. Held is visible and releasable.

**Non-Goals.** New numbers. Refusing instead of holding. The phone or window.

## Decisions

### The log is the source

Redaction bodies are retained — `destroy_bodies` excludes `REDACT` and
`REDACT_CANCEL` — precisely so a redaction goes on destroying matching events
that arrive later. That makes the log the one place where an origin's
redactions and their target counts are still legible after the notes are gone.

*Rejected:* the projection. It is derived and rebuilt, and the thing being
bounded is destruction of the projection's own contents. A ceiling that resets
when a projection is rebuilt is not a ceiling — and that is close to what the
broken query accidentally implemented.

### The window is around the redaction, not around now

`check` filters history to `now - WINDOW`. The call site passes the drain clock,
so a batch issued a month ago scores zero and proceeds.

The threat is a *rate*: a loop emits a thousand redactions in a second. That
cluster is a fact about when they were issued, and it stays true however long
the replica was offline. So `now` becomes the redaction's own timestamp and the
history is that origin's other redactions around it.

This also makes the bound deterministic. Two replicas draining the same batch at
different times reach the same verdict, which a wall-clock window could not
promise — and §1.1 makes this a receiver-enforced bound, so replicas disagreeing
about what is held would be worse than no bound.

### Release is local state, not an event

A release says "this device is satisfied". It is not a fact about the archive
and must not replicate: a compromised device that could emit releases would
release its own runaway. So it lives in `ops`, like the other local decisions,
and each replica decides for itself.

*Rejected:* a `redact_release` event kind. It would hand the attacker the key to
the lock.

### Refusing more than the cap

The fold takes the first 64 and drops the rest. Nothing records the drop, so a
redaction naming 100 reports success and leaves 36 intact. Held as malformed
instead — the event is kept, it changes nothing, and it is visible where other
unusable events are.

## Risks / Trade-offs

- **A legitimate bulk clear-out now needs a second command.** → That is the
  design's stated trade and the module doc argues it: costs a deliberate act to
  release, costs nothing to ignore.
- **Reading the log per drained redaction.** → Bounded by one origin's
  redactions and only on a path that is already doing destructive I/O.
- **A replica can be made to hold by a peer's clustered history.** → That is the
  bound working. The remedy is to look and release.

## Migration Plan

No schema change in the log. `ops` gains a table of releases. Existing archives
have no held redactions because nothing could be held.

## Open Questions

- **Should a held redaction expire into proceeding?** No, and worth stating:
  the whole value is that it costs nothing to ignore. A hold that times out into
  destruction is a delay, not a bound.
