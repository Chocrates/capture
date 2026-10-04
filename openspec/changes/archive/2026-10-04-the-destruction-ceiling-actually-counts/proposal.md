## Why

The only receiver-side bound on irreversible destruction does not exist.

DESIGN §1.1:

> **A redact names at most 64 targets**, and — because a loop emits a thousand
> redacts as easily as one — **receiving replicas rate-limit by origin**: beyond
> 256 targets per hour from a single origin, further redactions are **held
> pending explicit confirmation** rather than executed. The bound has to be
> enforced by the receiver, since the buggy or compromised device is exactly the
> one that will not enforce it on itself.

`m1-single-device-capture` task 9.3 — *"Implement the ingest-side rate limit —
256 targets per hour per origin, then held for explicit confirmation"* — is
marked done.

The limiter itself is correct and well tested. What feeds it is not.
`Store::redaction_history` is:

```sql
SELECT v.hlc_wall, COUNT(*) FROM note_versions v
 WHERE v.kind = ?1 GROUP BY v.event_id, v.hlc_wall
```

`note_versions` only ever receives `CAPTURE`, `EDIT` and `TOMBSTONE`
(`fold.rs:893`, `:959`, `:1773`); `REDACT` is handled in a separate pre-pass and
falls through to a no-op arm. **So the query returns zero rows, always.**
`ratelimit::check(&[], 1, now)` returns `Proceed` every time, `Drained.held` can
never be non-zero, and nothing reads it anyway. Measured: 400 redactions issued
within one minute produced a history of 0 rows, verdict `Proceed`, and 400
queued destructions.

Three further defects in the same query, each enough on its own: it is **not per
origin**, which the limiter's own module doc says is the thing that matters
(*"One device going wrong must not spend another's budget"*); it counts
**redactions rather than targets**, because `note_versions.event_id` is a
primary key so `COUNT(*) GROUP BY event_id` is always 1; and it has no test.

So one loop in one buggy build, or one device somebody else is holding, destroys
every note on every replica in a single round of drains. That is precisely the
failure §1.1's bound was written to prevent, and §7 mode 26h claims is handled.

## What Changes

- **The history comes from the log**, which is where redactions actually are.
  Their bodies are retained deliberately — `destroy_bodies` excludes them — so
  the targets are still countable long after the notes are gone.
- **Counted per origin, in targets.** Both are what the design says and what the
  limiter's own doc assumes.
- **The window is measured against the redaction's own timestamp**, not the
  clock at drain time. A runaway issues its thousand redactions in a second; if
  the window were measured from "now", that batch arriving after a month offline
  would count zero and sail through — which is the receiver case §1.1 says the
  bound exists for.
- **A held redaction is reported and can be released.** Holding is only a
  remedy if somebody can see it and act; `Drained.held` was incremented and read
  by nothing, and there was no way to say "yes, I meant it".
- **More than 64 targets is refused, not truncated.** The fold currently takes
  the first 64 and drops the rest with no record.

## Capabilities

### Modified Capabilities

- `event-log`: that destruction is bounded per origin, and what a held
  redaction is.
- `cli`: that held redactions are reported and releasable.

## Impact

- `crates/capture-store/src/log.rs` — an origin's redactions and their target
  counts.
- `crates/capture-store/src/projection.rs` — `redaction_history` goes; it is
  wrong in four ways and has no other caller.
- `crates/capture-store/src/session.rs` — the drain asks the right question.
- `crates/capture-store/src/ops.rs` — which redactions have been released.
- `apps/capture-cli` — reporting and releasing.

## Non-goals

- **Changing the numbers.** 64 and 256 are §1.1's and are not revisited.
- **Holding on the phone or in the window.** The reporting belongs everywhere
  eventually; the command line is where redaction's other controls already are.
- **Refusing rather than holding.** §1.1 is explicit that a rejection would put
  a gap in an origin's sequence, which this design treats as unrecoverable, and
  the limiter's module doc argues it at length.
