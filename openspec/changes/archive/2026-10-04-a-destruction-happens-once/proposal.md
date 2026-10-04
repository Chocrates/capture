## Why

A destroyed note stays in the destruction queue for ever, so every drain
destroys it again.

`purge_note` clears the text, the search index, the links, the mentions, the
list items and — since yesterday — the ambient row. It does not clear the
`redact_queue` row. The fold is right about this: once the body is gone it stops
setting `redaction_pending`, so a full rebuild produces no row. But `purge_note`
edits the projection in place, and nothing deletes what the fold would no longer
write.

Measured:

```
queued before drain: 1
drain 1: destroyed=1 held=0
queued after drain: 1
drain 2: destroyed=1 held=0
```

Two consequences, and the second is the one that matters.

**Every drain re-runs a full purge for every note ever redacted.** Record the
intent, attempt a snapshot, delete rows that are already gone, run an `UPDATE`
that changes nothing, **checkpoint the write-ahead log**, clear the intent. The
checkpoint is a real cost paid on every drain, for ever, multiplied by every
redaction the archive has ever carried out. And `Drained.destroyed` counts them,
so the overlay logs "overdue work completed" about work completed weeks ago.

**Cancelling a redaction that has already been carried out reports success.**
`cancel_redaction` looks for a non-cancelled queue row matching the id. The row
is still there, so it finds one, writes a `redact_cancel`, and prints
*"Cancelled. Nothing was destroyed."* The content is gone. Nothing was
recovered, and the person has been told the opposite — in the one operation
where being wrong cannot be walked back.

That second one is the local half of a finding already recorded: §1.1 says a
cancel arriving after destruction is *"recorded and reported rather than
silently discarded"*, and today it is neither. Removing the stale row makes the
local case honest, because `cancel_redaction` then finds nothing and says so:
*"it may already have been carried out"*, which is true.

## What Changes

- **Purging a note removes its queue row**, beside the other derived rows it
  already removes.
- **A destruction therefore happens once.** Later drains find nothing to do,
  `Drained.destroyed` stops counting old work, and the checkpoint is not paid
  again.
- **Cancelling after destruction says so**, rather than claiming a recovery
  that did not happen.

## Capabilities

### Modified Capabilities

- `projection`: that a carried-out destruction leaves the queue.

## Impact

- `crates/capture-store/src/projection.rs` — one delete in `purge_note`.

## Non-goals

- **Reporting a `redact_cancel` that arrives from a peer after destruction.**
  §1.1 asks for that too, and it is a different thing: an event that has to be
  recorded and surfaced rather than a row that should not be there. It stays
  recorded as its own finding.
- **Changing what `due` selects.** The queue is the right source; it simply had
  a row in it that the fold would not have written.
