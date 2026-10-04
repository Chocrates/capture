## Context

`purge_note` edits the projection in place rather than re-folding, so every
derived table has to be listed explicitly. Its comment records the same bug
being found twice — `mentions`, then `list_items`. `ambient` is the third.

## Goals / Non-Goals

**Goals.** The context goes when the note does.

**Non-Goals.** Changing what is collected. Removing the unread `title` column.

## Decisions

### Delete rather than blank

The other tables are deleted, not emptied, and a row of nulls keyed by the
target would still answer "this note existed and was captured somewhere". The
note row itself is kept because its id and its redacted-at are the record that
destruction happened; nothing in `ambient` plays that part.

### No filter on `by_context` instead

A filter would leave the material on disk and rely on every present and future
reader remembering to exclude it — which is the arrangement that produced this
and the two before it. Deleting the row makes the question unanswerable rather
than merely unasked.

## Risks / Trade-offs

- **A context query over destroyed notes silently returns fewer.** → That is the
  point, and the note is gone from every other surface already.
- **No schema bump, so existing projections keep rows for already-destroyed
  notes.** → True, and worth saying: they clear on the next rebuild, which
  `PROJECTION_SCHEMA` 10 has already queued for every archive that has not
  opened since. Named in the migration note rather than left implied.

## Migration Plan

No schema change. Rows left by redactions performed before this stay until the
next rebuild.

## Open Questions

- **Is the unread `title` column worth keeping?** It is written at capture and
  no reader selects it. Either something should show it or it should go; it is a
  separate decision from whether it leaks.
