# a-destroyed-series-leaves-no-occurrences

## Why

`purge_note` names every derived table it has to clear, because a purge edits
the projection in place rather than re-folding. It names eight. `occurrences` is
not one of them, so destroying a recurring series leaves its expanded rows
behind — and those rows are not only dates:

```sql
CREATE TABLE IF NOT EXISTS occurrences (
  ... title TEXT, location  TEXT, notes TEXT, ...
```

`title`, `location` and `notes` are filled from `OCC_EDIT` bodies — text
somebody wrote about one occurrence of a meeting — and the purge destroys those
bodies while leaving the copy. That is §1.1's stated purpose failing in the same
way it failed for `mentions`, `list_items` and `ambient`, each of which was
found surviving a purge and added to this function with a comment about it.

One honest qualification, found while writing this: **nothing can author an
`OCC_EDIT` today.** `Kind::OCC_EDIT` has a body type, a fold arm
(`apply_occ_edit`), three projected columns and no writer anywhere — not in
`mutate`, not in the CLI, not in `capture-ffi`, not in the window
(`ipc::set_occurrence` writes `OCC_DONE`/`OCC_UNDONE` only), not in the Android
app. So in a real store those three columns are always NULL, and what actually
survives a purge today is the schedule and the completion state: every expanded
occurrence of the destroyed series, and which of them you had ticked off.

That is still information about a capture that no longer exists, and it is still
a row keyed by a destroyed note that the guard claims to check. But it is not
yet a text leak, and saying it was would be overstating the defect. It becomes
one the day per-occurrence editing is wired up, which is why the delete and the
fixture both cover those columns now.

This is the seventh table found this way, and the point is that it should not
have been found by reading at all. `tests/purge_leaves_nothing.rs` exists
precisely so the next one is caught by running: it enumerates every table with a
`tgt` column **at runtime** and fails if any holds a row for a destroyed note.
It passes here, because its fixture note is

```
#vault lunch with @sarah about ^acme on 2026-11-03: milk, eggs, bread
```

— a one-off dated capture. `occurrences` is empty, so the enumeration has
nothing to find. The guard is right and its fixture is too thin, which is worse
than the missing delete: the delete is one line, and a guard that cannot fail is
a guard that will let the eighth table through as well.

## What Changes

- `purge_note` deletes from `occurrences`.
- The guard's fixture gains a **recurring series with an edited occurrence**, so
  `occurrences` is populated — with a title, a location and notes — when the
  enumeration runs. This is the change that matters: it is what makes the guard
  able to fail.
- Whether a series merely *queued* for destruction also leaks its occurrence
  titles is settled by test rather than by argument. `occurrences` is expanded
  from the **promotion**, not from the note's text, so it sits outside the
  `readable_text` guard that now covers the text-derived tables — the withholding
  there may already be provided by the agenda's own filters, or may not be
  provided at all.
- Any other table the widened fixture turns up is fixed in the same pass, since
  the fixture is the thing being corrected.

## Capabilities

### Modified Capabilities

- `projection` — a destroyed series leaves no occurrences, and the guard that
  checks this is exercised against a note that has some.

## Non-goals

- **No re-fold in `purge_note`.** Editing in place is why every derived table
  has to be named, and re-folding instead is the structural fix — it would make
  this category of defect impossible rather than caught. It is also much heavier
  on the one path that must not fail halfway, and it deserves its own change with
  the crash behaviour thought through. Recorded as the open question it has been
  since the fourth table.
- **No change to what an occurrence is.** Expansion, exceptions, splits, DST
  marking and the inheritance of exceptions across a split are all unaffected.
- **Not the text-derived tables.** `mentions`, `list_items` and `links` are
  handled by `a-queued-redaction-withholds-everything`, which guards them on one
  condition at the point they are written.
