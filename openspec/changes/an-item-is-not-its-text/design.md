## Context

`list_items` is `PRIMARY KEY (tgt, ord)`. Items already belong to the capture
that wrote them, in written order, and promotion is independent — so a task can
already carry items and so can a page. The model is better than the surfaces
admit.

What is wrong is addressing. Every surface reaches items by *tag*, and every
completion reaches an item by *text*:

- `ItemBody { nonce, item: String }` — the item is its words.
- `tick(tag, item, done)` finds the row by matching those words.
- `list(tag)` gathers across captures, so one capture's list is indistinguishable
  from another's.

Three defects follow, all reproduced: a reword discards a tick, two items
reading the same are untickable, and a list capture promoted to a task appears
inside its own list and is counted with its contents.

## Goals / Non-Goals

**Goals.** Completion survives rewording. Duplicate items are tickable. A
capture's checklist is reachable as its own. Items without a tag. A collection
that does not contain the capture that wrote it.

**Non-goals.** Hand reordering. Nested checklists. Tag identity. More than one
checklist per capture.

## Decisions

### Position *and* text, reconciled — because neither alone is an identity

This is the hard part and it deserves being said plainly: **an item has no
identity to find.** Items are re-derived from a mutable string on every fold.
There is nothing minted at capture time to point at, and nothing can be, because
the capture body is frozen and does not carry item ids.

So every scheme here is a reconciliation, not a lookup, and the question is
which failure is least bad.

- **Text alone** (today) loses a tick whenever a word is corrected — the most
  ordinary edit there is.
- **Position alone** loses nothing to rewording and gets *inserting* wrong in
  the worst way: adding one item at the top shifts every later position, so
  every tick below it moves to the wrong row. Silent, and worse than losing it.

So `item_done` carries **both**, and the reconciliation runs in the store, in a
fixed order.

**Not in the fold**, and for the reason the entity mutes could not be settled
there either: matching a tick against the items of a capture means expanding its
text, `list_of` lives in `capture-parse`, and `capture-parse` already depends on
`capture-core`. So the fold carries the ticks *raw* — position, text and state,
in event order — and `write_into` reconciles them where it already expands the
items. One place, the only place that has both halves.

1. **Exact** — same position *and* same text. The untouched case.
2. **Unique text** — the words appear exactly once, at a different position.
   Covers insertion and deletion above the item.
3. **Position in range** — the words are gone but the slot is there. Covers
   rewording.
4. Otherwise **held with a reason**, never applied to something else.

Each rule is the answer to one edit. Taken together they cover reword, insert,
delete and reorder-by-rewriting individually.

**What they do not cover, and this is the residual cost:** rewording an item
*and* inserting another above it in the same edit. Rule 1 misses, rule 2 misses
(the words changed), rule 3 fires on a position that now holds a different item,
and the tick lands on the wrong row. Rarer than either edit alone, and the only
alternative is to hold every ambiguous case — which would discard ticks for the
common edits to avoid being wrong in an uncommon one.

*Rejected:* hashing `(tgt, ord, text)` into an id. It is text as identity with
extra steps, and fails the reword case it is supposed to fix.

*Rejected:* a `list` event per capture carrying minted item ids, written after
the capture. It makes every checklist two events, puts a write on a path that
§3.1 says does not wait for anything, and leaves the items of a capture written
by an older build with no ids at all.

### The text stays in the body

`ItemBody` keeps `item` and gains the position. The text is what makes rule 2
work, and it is also what keeps `capture log` legible — an event saying
"item 2 done" and nothing else is unreadable by a person trying to understand
their own archive.

### Kind numbers do not change

`item_done` is 28 and `item_undone` is 29, and they stay. The body gains a field
at a new index, which is the one permitted evolution — frozen means fields may
be added, never moved or reinterpreted. An older build reading a newer event
gets the text it already understood.

### An untagged checklist is an empty tag, not a new table

`list_items.tag` is `TEXT NOT NULL` and the schema may add tables but never
columns. An untagged checklist stores the empty string, and a collection query
asks for a non-empty tag.

*Rejected:* a separate `checklist_items` table for untagged ones. Two tables for
one concept, and every reader would have to union them.

### A collection excludes the capture that owns the items

`list(tag)` gathers tasks carrying the tag beside items of captures carrying it.
The rule it needs is one line: a task that *is* the capture whose items are
already listed is not a separate thing beside them.

### Syntax for an untagged checklist

`: milk, eggs, bread` — the same anchored form with the tag omitted. Same three
guards: anchored at the start, something follows the colon, and where a tag is
written it is a single slug.

Deliberately not inferring a list from prose. `buy milk, eggs and bread` is a
sentence, and §3.1.5's rule is that a match firing inside ordinary writing is
worse than no match at all.

## Risks / Trade-offs

- **The reword-and-insert case lands a tick on the wrong row.** → Named above,
  accepted, and the alternative is worse for the common edits. Worth revisiting
  if it ever actually happens to somebody.
- **The fold no longer answers which items are done.** `Note.items` keyed by
  text is replaced by the raw ticks in order, so anything reading it has to go
  through the projection. That is where every surface already reads items from.
- **Existing `item_done` events carry no position.** → Read by text against the
  capture they target: applied where it still matches, held with a reason where
  it does not. The one place the old encoding cannot be carried forward exactly,
  and the proposal says so.
- **`PROJECTION_SCHEMA` 8 → 9 rebuilds every store.** → The designed migration,
  ninth time, and `state.db` is disposable.
- **A leading `:` may be something somebody writes.** → Anchored and requiring
  content after it, same as the tagged form. Measured against real captures
  before it ships rather than assumed.

## Migration Plan

- **Log:** nothing rewritten. `ItemBody` gains a field; both shapes decode.
- **Projection:** 8 → 9 with its changelog line, rebuilding on next open.
- **Rollback:** reverting leaves newer `item_done` events carrying a field an
  older build ignores, falling back to text — which is exactly the old
  behaviour.

## Open Questions

- **Is `:` the right prefix for an untagged checklist?** It is the smallest
  change to a form that already works. A different marker is a surface decision
  and belongs to whoever has to type it daily.
- **Should a collection show which capture each item came from?** The spec asks
  only that it be clear they are not one list. A heading per capture is the
  obvious answer and is worth looking at rather than deciding here.
- **Does an untagged checklist belong in any collection at all?** Assumed not —
  it has no tag, so there is nothing to gather it under, and it is reachable
  from its capture.
