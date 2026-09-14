## Context

DESIGN.md §3.6 says lists are tags with a display order and has said so since M2; the promotion
spec already carries "optional list membership and optional ordering" and a scenario asserting a
task may sit in several lists at once. None of it is implemented, and there is no syntax that
produces a list in the first place.

What exists and is worth reusing:

- **Sigils** are tier 1 (§3.1.1, §3.1.2): explicit, unscored, and they beat any inference in the
  same line. `#tag` is already parsed at capture time.
- **Recurrence** already solves "one event, many independently completable things": a series
  expands into occurrence rows in the projection, each completed by `occ_done`, and a rebuild
  reproduces the expansion exactly.
- **`mentioning(sigil, value, now)`** in `projection.rs` already answers "what carries this tag",
  and backs `capture mentions`.
- **Promotion is additive** (§3.1.3) and never rewrites the capture.

What does not exist: any way to say "these are items", any per-item state, and — on the phone —
any read by tag at all. `capture-ffi` exposes `tag` and `untag` for writing and nothing for
reading back by tag, and no promotion-to-task.

## Goals / Non-Goals

**Goals:**

- One line, one gesture, one event, several items.
- An item can be ticked off, and what that records survives a projection rebuild.
- A list is a query over a tag, so a task tagged by hand joins the list it belongs to without
  being copied into it.
- The phone can show and tick a list, which is the whole point — the phone is the device standing
  in the shop.
- No new vocabulary, no new dependency, no new event kind beyond the promotion that already
  exists.

**Non-Goals:**

As stated in the proposal: reordering by hand, quantities and units, appending to an existing
list by a later capture, nesting, voice-dictated lists, and a dedicated desktop pane. Each is a
separate question rather than a smaller version of this one.

## Decisions

### Items are expanded into the projection, not written as captures

One gesture stays one event. The alternative — the capture writing one note per item — was
considered and rejected: it would be the first thing in this system where a single gesture
appends many events, it makes the original line either a parent note or garbage, and it means the
log's size is a function of how many commas somebody typed.

Expansion follows the occurrence machinery exactly (§3.5), which is the precedent that makes this
cheap: derived rows, discarded and recomputed by a rebuild, never in the log.

The cost, accepted knowingly: **an item is not a note.** It cannot be edited, linked, given its
own tags or opened as a page. If that turns out to be wanted, the upgrade path is that an item
*becomes* a note by being promoted — which is already what ticking does, and the same door opens
for editing later without changing anything decided here.

Unlike occurrences, items need **no rolling horizon**. A list's items are all of them, known at
parse time, bounded by the line's length. The horizon exists for recurrence because a series is
infinite; a list is not.

### Completion state lives in the log as a task, and is matched back by text

This is the load-bearing decision and the reason "promote then complete" is right rather than
merely convenient.

An expanded item's identity is derived — capture plus position — and derived identity is fragile
under editing: change `#groceries: milk, eggs` to `#groceries: oat milk, eggs` and every position
after the edit shifts. If completion were stored against that identity, editing a list would
silently un-tick things, or worse, tick the wrong ones.

So completion is **not** stored against the item. Ticking appends a promotion to a task carrying
the item's text and the list's tag — an ordinary event, in the log, with a time on it. The
projection then matches an expanded item to a completing task by **(tag, item text)**.

What this buys:

- Completion survives a rebuild, because it is not derived.
- Completion survives an *edit* of the list capture, because it was never bound to a position.
- "Bought the milk" is a fact with a timestamp, which is what makes a list worth keeping rather
  than a row that vanished.
- It is idempotent for free: the match is by text, so completing twice finds the same task.

The cost: **two items with the same text on one list are indistinguishable.** `#groceries: milk,
milk` gives one tickable item as far as completion is concerned. The CLI spec requires this be
reported as an ambiguity rather than guessed at. It is a real limitation and it is the honest
consequence of anchoring state in the log rather than in a derived index.

### Gathering: the union, deduplicated by that same match

Reading a list returns expanded items and tasks carrying the tag, as one sequence. A task whose
text matches an expanded item on that list **is** that item and is returned once — otherwise
every ticked item would appear twice, once as an item and once as the task that ticked it.

Items first in display order, then loose tasks newest first. Items have a written order and
tasks do not, and interleaving them by time would scramble the order somebody deliberately typed.

### The syntax adds punctuation, not vocabulary

`#groceries: milk, eggs, bread`. The sigil table gains no character and §3.1.5's closed list
gains no word, so tier 2's tests stay exhaustive over the same vocabulary.

A `list` head word was considered — `REC_HEADS` is the precedent, and it would be more
discoverable. Rejected because §3.1.5 makes growing the vocabulary "a deliberate edit with tests
attached", and because `#groceries:` says the same thing using a marker already in the language.
The colon is doing the work, and it is doing it behind a sigil that already means "file it under
something".

**Anchored at the start**, which is the whole guard. A sigil anywhere else is an ordinary tag
whatever follows it, so `ask sarah about #groceries: she has it` and `#standup at 9:30` both stay
prose. This is §3.1.5's guard pattern — a match that would fire inside ordinary writing is worse
than no match.

### The FFI gains list operations, not primitives

Task 2.1 says the binding is "operation-shaped, not type-shaped". So the phone gets a call that
reads a list and a call that ticks an item — not a general `mentioning()` and a general `task()`
for Kotlin to compose. Composing them on the Kotlin side is Kotlin deciding something, which
`make android-decides` exists to prevent.

### The phone reaches a list without a new tab

The tab row already overflows on a real Pixel 8 Pro — seven tabs, "Timeline" wrapping mid-word
and "Health" clipped. An eighth tab makes a known bug worse.

A list opens from the tag itself: tapping a tag on a note page, and from search. That is also the
more honest surface, because a list is a query and tags are where queries start.

## Risks / Trade-offs

- **Two identical items on one list cannot be ticked apart** → Reported as an ambiguity rather
  than guessed at, in both the CLI and the phone. Accepted as the price of log-anchored state.

- **An item containing a comma cannot be written** → `milk, 2%` is two items. No escaping is
  proposed; the workaround is a second list capture or rewording. Revisit only if it actually
  bites.

- **Captures written before this change become lists when the projection is rebuilt** → Intended,
  and specified. It is additive (§3.1.3): the text is untouched, and the worst case is a note
  that already carried a leading tag and a colon now showing items. Worth watching for on the
  first rebuild against a real archive.

- **Editing a list capture re-expands its items** → Completion survives because it is matched by
  text, but an item whose text was *changed* loses its tick, which is arguably correct: it is a
  different item. Called out so it is a decision rather than a surprise.

- **The list is unbounded in item count** → A pathological line with a thousand commas expands to
  a thousand rows. Bounded in practice by what someone will type; if it needs a cap, the cap
  belongs at parse time and should be stated in the spec rather than discovered in the
  projection.

- **`capture done` is already overloaded** → It takes `<name>` and `<name> <occ>`. See Open
  Questions.

## Migration Plan

None required. The projection is rebuildable from the log (§1.4), so items appear on the next
fold. No event is rewritten, no schema migrates, and nothing already captured changes meaning in
the log — only in the derived view, which is what derived means.

Rollback is reverting the code: the projection rebuilds without list rows and the only events
this change ever appended are ordinary task promotions, which remain valid and readable as
tasks.

## Open Questions

1. **`capture done groceries milk` collides with `capture done <name> <occ>`.** The occurrence
   form takes a start time, so the two are distinguishable in practice by whether the second
   argument parses as a date — which is exactly the kind of fragile disambiguation §3.1.5's
   guards exist to avoid. A separate verb (`capture tick` / `capture untick`) is the obvious
   alternative and costs a word of surface. **This needs deciding before the CLI spec is
   implemented, and resolving it may amend that delta spec.**

2. **Should a second `#groceries:` capture append to the first list or stand beside it?** The
   proposal defers this and the specs make both captures' items appear under one tag when read,
   which is the "stand beside" answer. If appending is wanted later it is a change to what a list
   capture *means*, not to how items are expanded.

3. **Does a ticked item leave the list after some time?** A grocery list that accumulates every
   item ever bought is not usable at the shop. The spec keeps completed items on the list, which
   is right for the log and possibly wrong for the surface. Likely answer is that the *view*
   hides completed items behind a toggle rather than the query dropping them — but it is not
   decided, and it is the first thing that will be noticed in real use.
