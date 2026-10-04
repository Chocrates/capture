# Design

## The missing delete is the small half

`purge_note` clears eight tables: `links`, `mentions`, `list_items`, `ambient`,
`redact_queue`, `promotions`, `promotion_at` and the two full-text rows. Three of
those carry a comment recording that they were found surviving a purge — the
third one says it outright: *"the third table to be found surviving a purge,
after the two above, and for the same reason — a purge edits the projection in
place, so every derived table has to be named here or it is simply missed."*

`occurrences` is the seventh table found by reading. Adding it is one line.

## The fixture is the half that matters

`tests/purge_leaves_nothing.rs` was written so that reading would stop being how
these are found. It asks SQLite for every table with a `tgt` column, destroys a
note, and fails if any of them still holds a row — with an allow-list where each
survivor carries a written reason. A table added next year is covered the day it
is added.

It passed. The fixture is

```
#vault lunch with @sarah about ^acme on 2026-11-03: milk, eggs, bread
```

which promotes to a one-off dated capture. `occurrences` is only ever populated
from a `Promoted::Series`, so the table was empty, the `COUNT(*)` was zero, and
zero reads identically to clean.

So the guard has a failure mode of its own: **a table the fixture does not
populate is silently exempt.** Every table it covers is covered only to the
extent the fixture reaches it, and nothing says which ones those are.

### Emptiness has to be a failure

The fix is to make the test assert coverage as well as cleanliness: count the
rows for each enumerated table *before* the destruction, and fail if a table the
test claims to cover held nothing to begin with. Then a fixture that stops
reaching a table is a test failure that names the table, rather than a quiet
reduction in what is checked.

This inverts the usual shape of such a test, where a missing row is a pass. That
is the point: the question is not "is this table clean" but "did destroying a
note clean this table", and those differ exactly when the table was never dirty.

Two consequences worth stating:

- A table that genuinely cannot be populated by any single capture will have to
  be named, with a reason, in the same shape as the existing allow-list. That is
  the right outcome — it is a written claim about coverage rather than an
  accident of the fixture.
- The existing `MAY_SURVIVE` list and this new one answer different questions —
  "this row is allowed to remain" and "this table is not reachable from one
  capture" — so they stay separate. Merging them would let a reason for one
  excuse the other.

## The queued window is a separate question, settled by test

`occurrences` is expanded from `promotion.as_` being a `Promoted::Series`, not
from `note.text`. So it is untouched by the `readable_text` guard that now covers
the text-derived tables, and a series *queued* for destruction may still have
occurrence rows with their titles.

Whether that is visible is not obvious: the agenda may already exclude the note
through `ALERTABLE` or the `NOTE_COLUMNS` withholding, in which case the rows
exist and nothing draws them. That is a meaningfully different situation from
mentions, where the entity page read the row directly.

So this is checked by writing the test first and seeing what it says, rather than
by deciding in advance. If the rows are reachable, the withholding belongs here
on the same footing as the others; if they are not, that is worth an assertion
either way, because "unreachable by today's queries" is exactly the kind of
safety that disappears when somebody adds a query.

## Open question, now at its fifth table

**Should `purge_note` re-fold instead of editing in place?** Editing in place is
the root cause of this entire category: seven tables found one at a time, three
of them by hand, each with a comment explaining that it was missed because the
function has to name them all. Re-folding would make the category impossible.

Against it: the purge is the one path that must not fail halfway, a re-fold is
much heavier than a handful of deletes, and the crash behaviour would need
thinking through properly — `resume` exists because a purge can be interrupted,
and a re-fold mid-purge is a different recovery problem from a delete mid-purge.

Still deferred, and this is the fifth table at which it has been deferred, which
is itself an argument. Recorded rather than acted on because doing it carelessly
on this path is worse than the defect it prevents.
