## Why

A list of things to buy is the most ordinary capture there is, and today it has nowhere to go.
`buy milk` is a task verb with no day, which §3.1.5 scores as a suggestion and nothing more, so
it stays an ordinary note; ten of them are ten unrelated notes that no surface gathers. The
alternative available right now — one note reading "milk, eggs, bread" — cannot record that the
milk was bought, which is the only state a shopping list has.

DESIGN.md §3.6 already decided the shape and nothing implements it: *"Lists are just tags with a
display order, so a task can sit in several without duplication."* This change builds that
sentence.

It is worth doing now rather than later because the phone is the device standing in the shop,
and M4 has just put a capture field on its home screen. A list is the first thing that surface
is asked for and the first thing it cannot do.

## What Changes

- **A capture whose text opens with a tag sigil and a colon is a list.** `#groceries: milk,
  eggs, bread` writes one capture event and names three items. The form is tier 1 — an explicit
  sigil, which §3.1.1 makes always-recognised and never-required, and §3.1.2 makes a winner over
  any inference in the same line.
- **Items are expanded into the projection, not into the log.** One gesture stays one event.
  Items become projection rows carrying their own identity and completion state, exactly as a
  recurring series expands into occurrences (§3.5) that complete independently.
- **An item can be ticked off.** Completing one promotes it to a task and marks it done, so
  "bought" is a fact in the log with a time on it rather than a row disappearing. Reopening is
  the same path backwards.
- **A list is a query, not a container.** `#groceries` gathers both the items named in list
  captures and any task carrying that tag, which is what §3.7's "collections are saved queries,
  not folders" requires and what makes §3.6's "a task can sit in several without duplication"
  true rather than aspirational.
- **The command line and both windows can read and complete a list.** On the phone this means
  `capture-ffi` gains the two calls it is missing: a read by tag, and the promotion that makes an
  item a task.

Nothing about existing captures changes. A note that happens to contain a colon is not a list;
the form is anchored at the start and behind a sigil for exactly that reason.

## Capabilities

### New Capabilities

- `lists`: what a list capture is, how its items are named and expanded, what a list gathers, and
  what completing an item means.

### Modified Capabilities

- `recognizer`: a new tier-1 form. The sigil table gains no character and the closed vocabulary
  (§3.1.5) gains no word, but `#tag:` followed by a separated sequence is newly recognised, and
  the guards that keep a colon inside prose from being a list need stating.
- `projection`: list items become expanded rows alongside occurrences, rebuildable from the log
  and queryable without reading the text.
- `promotion`: the task requirement already carries "optional list membership and optional
  ordering" and already says a task may sit in several lists without duplication — specified in
  M2 and never implemented. Nothing there changes. What is added is what completing a list item
  means: a promotion rather than a deletion.
- `cli`: reading a list and completing an item from the command line.

## Impact

**Code.** `capture-parse` (the list form and its guards); `capture-store` — `projection.rs`
(expansion, `mentioning` extended to gather items), `promote.rs` (item → task), `mutate.rs`
(completion path); `capture-ffi` (a read by tag and a task promotion, neither currently exposed);
`apps/capture-cli`; `apps/capture-desktop`; `android` (a list surface).

**No new dependencies.** The parse is over the existing tokenizer and the expansion follows
recurrence's own machinery.

**Migration.** None. The projection is rebuildable from the log (§1.4), so items appear on the
next fold and no existing event is reinterpreted — a capture written before this change that
happens to match the form becomes a list when the projection is rebuilt, which is the intended
behaviour of an additive recognizer (§3.1.3) and is called out in the specs.

## Non-goals

Deliberately deferred, each because it is a separate question rather than a smaller version of
this one:

- **Reordering by hand.** §3.6 says lists have a display order; this change gives items the order
  they were written in and no way to change it. A reorder gesture is a UI question on three
  surfaces and wants its own milestone.
- **Quantities and units.** `2x milk`, `500g flour`. This is vocabulary growth, which §3.1.5
  requires be a deliberate edit with tests attached, and it is not needed to buy milk.
- **Adding to an existing list by capture.** A second `#groceries: bread` capture is its own list
  that the same tag gathers; whether a later capture should *append* to an earlier list rather
  than stand beside it is a real question and is not answered here.
- **Nested lists, and lists of anything but text items.**
- **Voice-dictated lists.** Hold-to-talk produces a recording and a transcript arrives through
  enrichment (§2.5); whether a transcript can be a list capture is deferred until enrichment is
  built.
- **A dedicated list surface on the desktop.** The command line and the phone are enough to use
  one; the desktop gets the query and not a new pane.
