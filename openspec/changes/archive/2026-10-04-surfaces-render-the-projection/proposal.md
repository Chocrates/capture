## Why

Every surface draws the captured line and nothing else. A task, an event, a
recurring series and a list all render identically — as the text somebody
typed:

```
2026-09-15 Tue
  standup-every-weekday-etcd8b88ym      standup every weekday at 9
  call-dentist-tomorrow-y9903tamvs      call the dentist tomorrow
  dentist-thursday-x55pp09c32           dentist thursday 2pm
  groceries-milk-eggs-bread-0bkt3ewr4d  #groceries: milk, eggs, bread
```

The projection knows each of those is a different thing. It holds a kind, a set
of fields, a rendered descriptor, expanded occurrences and expanded list items
for them, and none of it reaches a screen. `NoteSummary` — the record every
surface draws from — carries `text` and no descriptor at all. The only consumer
of a descriptor anywhere is `capture tasks` on the command line.

The cost is sharpest on lists, which is where it was noticed: a list capture
appears as `#groceries: milk, eggs, bread`, sigil and commas included, and the
items it names are reachable only by opening the note and pressing a button.
The syntax of §3.1.1 is an accelerant for *writing*. Reading it back is the
reader doing the recognizer's job by eye.

The same is true more quietly everywhere else. "call the dentist tomorrow" is a
task with a due date and reads as a sentence; "standup every weekday at 9" is a
series with a rule and reads as a sentence. §3.1 makes types promotions rather
than choices — and then never shows anyone what was promoted.

## What Changes

- **Every surface renders what a capture *is*, with what was typed still
  visible.** A list draws as its items, tickable in place. A task draws with its
  state and due date. An event draws with its time. A series draws with its
  rule. The captured line stays on the row as secondary text.
- **`NoteSummary` carries the projected reading**: the promotion's kind, its
  descriptor, and — for a list — its items. Today it carries none of these, so
  no surface *can* render the projection even if it wanted to.
- **A list is tickable wherever it appears**, rather than behind a button on a
  note page. The list screen added by `lists` stays for a tag-wide view across
  several captures; what changes is that the capture itself renders as a list.
- **The note page shows the structured half beside the text**, which `browse`
  already requires of the desktop and which no surface currently does.

## Capabilities

### New Capabilities

None. This is the existing `browse` requirement applied to surfaces that never
implemented it, plus the projection carrying what it already computes.

### Modified Capabilities

- `browse`: the note page requirement is extended from "the structured half
  beside the text" on one surface to every surface, and from the note page to
  the timeline.
- `projection`: promoted fields are already required to be queryable without
  reading the text; this adds that the rendering a surface draws is one of
  them, so two surfaces cannot render the same note differently.

## Impact

**Code.** `capture-ffi` (`NoteSummary` gains kind, descriptor and items; the
timeline and note reads fill them); `android` (timeline rows, note page);
`apps/capture-desktop` (the same, through its IPC); `apps/capture-cli` — see
below.

**Not the command line.** §8.6 makes it the test harness and the surface you
reach for when something is wrong, which is exactly the "viewing history or
debugging" case where raw text is the thing you want. `capture timeline`
continues to print what was captured, and `capture list` already renders the
projected form.

**No schema change.** Everything being surfaced is already computed and stored.

## Non-goals

- **Editing structure from a row.** Correcting a wrong promotion stays a
  deliberate act on the note page (`capture task`, and the desktop's existing
  correction path). A timeline that could silently repromote things while
  scrolling is a different and worse design.
- **Hiding the captured text.** §3.1.4 wants text, fields and descriptor all
  visible so drift between them stays catchable — "the text says every day at
  7, the fields say weekdays". A rendering that showed only the fields would
  hide exactly the disagreement the third thing exists to reveal.
- **A descriptor for an ordinary note.** `Promoted::Note` renders empty on
  purpose: a note's content *is* its text, and inventing "untitled" or "no date
  set" would add facts the capture did not contain (§3.1.3).
- **Redesigning the timeline.** Rows gain structure; the surface keeps its
  shape.
