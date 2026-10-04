## Context

`purge_note` edits the projection in place. Every derived table it should clear
has to be named there, which is why `mentions`, `list_items` and `ambient` were
each found surviving a purge in turn. `redact_queue` is the fourth.

## Goals / Non-Goals

**Goals.** A destruction happens once. Cancelling afterwards is honest.

**Non-Goals.** Reporting a peer's late `redact_cancel`. Changing what `due`
reads.

## Decisions

### Delete in the purge, not a filter in `due`

`due` could skip targets already destroyed, and that would leave the row there
to be skipped by every future reader that remembers to — the arrangement that
produced the three before this. The fold's own answer is that the row should not
exist, so the purge makes the projection agree with it.

### Why it is safe to delete something the fold derives

A later full rebuild recomputes the queue from the log and will not re-add it:
the body is gone, so `redaction_pending` is `None`. An incremental write of the
same note clears and rewrites its rows, reaching the same place. Deleting here
only brings forward what both paths already conclude.

## Risks / Trade-offs

- **A cancellation that would have "succeeded" now fails.** → It never
  succeeded at anything; it reported success over destroyed content. Failing is
  the honest outcome and the message already describes it.
- **`Drained.destroyed` will report smaller numbers.** → Because it stops
  counting work finished weeks ago.

## Migration Plan

No schema change. Rows left by destructions carried out before this clear on the
next rebuild, which `PROJECTION_SCHEMA` 11 has already queued.

## Open Questions

- **Should a purge be the only thing that edits the projection in place?** Four
  tables have now been found surviving one. A purge that re-folded the affected
  note instead would get the whole answer from the fold, at the cost of being
  much heavier on the one path that must not fail halfway.
