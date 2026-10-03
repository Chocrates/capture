## Why

Three defects, one cause, and it is the cause `entity-pages` just finished
removing from mentions: **a name is being used as an identity.**

An item is identified by the words written for it. `ItemBody { item: String }`
carries the text, and `tick` finds the row by matching it. So:

```
$ capture tick shop eggs
  [ ] milk   [x] eggs   [ ] bread          2 of 3 left.

$ capture edit <slug> '#shop: milk, free-range eggs, bread'
  [ ] milk   [ ] free-range eggs   [ ] bread    3 of 3 left.
```

**The tick is gone.** The `item_done` event is still in the log; it no longer
matches anything, so it applies to nothing and nothing says so. Correcting a
word on a shopping list silently discards what you had already bought. §6 rates
a state that cannot be reversed as the failure worth avoiding, and this is worse
— it is a state that is *lost* by an unrelated gesture.

The same cause makes two items reading the same untickable at all. `tick`
answers `Ambiguous` and refuses, because text is all it has to go on, and
"milk, milk" is a perfectly ordinary thing to write when two shops are meant.

And a **tag** is being used as a list's identity. There is exactly one
`#groceries` list in an archive, for ever. Two shopping trips are one list. The
tag cannot be renamed without the list forking, and two lists cannot be merged —
precisely the four failures §4.3 names for entities, in a second place.

The overlap produces a list that contains itself:

```
$ capture task <the list capture>
$ capture list groceries
  [ ] milk   [ ] eggs   [ ] bread
  [ ] #groceries: milk, eggs, bread          ← the capture, inside its own list
  4 of 4 left.
```

`list(tag)` unions *items of captures tagged X* with *tasks tagged X*. The
comment above that union reasons carefully about a task that is a separate thing
beside the items — and does not anticipate the capture that owns the items
becoming one.

Underneath all of it, the model is already better than the surfaces admit.
`list_items` is `PRIMARY KEY (tgt, ord)`: **items already belong to the capture
that wrote them**, in written order. Promotion is independent, so a task can
already carry items and a page can already carry items. What is wrong is that
every surface addresses them by tag, and every completion addresses them by
text.

## What Changes

- **An item gains an identity that is not its words.** `item_done` and
  `item_undone` name the item by its position in the capture that wrote it,
  which is stable across editing the words and is what `list_items.ord` already
  records.
- **Editing the text keeps what was ticked.** Rewording an item leaves its
  completion alone. Deleting one drops its completion with it; adding one
  arrives unticked.
- **Two items reading the same are two items.** `Ticked::Ambiguous` stops being
  reachable, because nothing is matched by text any more.
- **A checklist and a collection stop being the same word.** A *checklist* is
  the items of one capture — bounded, ordered, owned. A *collection* is
  everything carrying a tag — unbounded, a query, owned by nobody. Both are
  useful; conflating them is what produces a list that contains itself.
- **A capture's own items are reachable as its own list**, without a tag.
- **Items no longer require a tag to exist.** A task with sub-items does not
  want a label, and today the only syntax that produces items is `#tag:`.
- **`list(tag)` stops including the capture that owns the items**, which is the
  self-inclusion above.

**BREAKING (projection only):** `PROJECTION_SCHEMA` 8 → 9. No event is
rewritten.

**BREAKING (behaviour, deliberately):** `item_done` events already in a log name
their item by text. They are re-read against the capture they target, so an
existing tick survives where its words still match and is otherwise held with a
reason rather than silently dropped. This is the one place where the old
encoding cannot be carried forward exactly, and saying so is better than a
completion quietly moving to the wrong row.

## Capabilities

### New Capabilities

- `checklist`: what the items of a capture are, how one is identified, what
  survives an edit, and how a checklist differs from a collection.

### Modified Capabilities

- `event-log`: `item_done` and `item_undone` identify an item by position. Both
  keep their kind numbers; the body gains a field and the old shape is read.
- `projection`: `list_items` is specified, including that items belong to a
  capture and that a redacted capture has none.
- `browse`: a capture's checklist is reachable from the capture, and a tag's
  collection stops claiming the capture that wrote the items.
- `recognizer`: items may be written without a tag.

## Impact

- `crates/capture-core/src/body.rs` — `ItemBody` gains a position; the text
  stays, so an old event is still readable and a new one is still legible in a
  log dump.
- `crates/capture-core/src/fold.rs` — items keyed by position, and the
  compatibility read for events written before this.
- `crates/capture-core/tests/golden.rs` — a vector for the new shape, and the
  old one kept, because both are now things that exist.
- `crates/capture-parse/src/lib.rs` — `list_of` accepts items with no tag.
- `crates/capture-store/src/{projection,promote}.rs` — `tick` by position,
  `list(tag)` without the owner, a capture's own checklist, schema 8 → 9.
- `crates/capture-ffi`, `android/`, `apps/capture-cli`, `apps/capture-desktop` —
  the surfaces, which currently all address items by text.

## Non-goals

- **Reordering items by hand.** `TaskFields.order` exists and nothing writes it;
  giving items a hand order is a separate argument and a separate surface.
- **Nested checklists.** An item is a line of text, not a capture. Items with
  their own items is a tree, and §3.7 spends its length on why this system does
  not file things into trees.
- **Renaming or merging tags.** `entity-pages` deliberately left tags with no
  identity of their own, and that stands. What changes here is that a tag stops
  being asked to *be* a list's identity.
- **Multiple checklists in one capture.** One capture, one list, in written
  order. Two lists is two captures.
