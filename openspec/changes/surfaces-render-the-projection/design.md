## Context

`NoteSummary` is the record every surface draws a row from. It carries `slug`,
`alias`, `text`, timestamps, `pinned`, `muted_until_ms`, the two redaction
flags, and (since `lists`) `list_tag`. It carries no kind, no descriptor and no
fields.

So no surface can render the projection even if it wanted to — the information
never leaves the store. The projection has computed it the whole time:
`promotions` holds `kind`, `fields`, `descriptor`, `due`, `done`, `start_ms`;
`occurrences` and `list_items` hold the expansions.

`descriptor.rs` is explicit that the rendering belongs to the projection and not
to a view: "a descriptor that differed between two surfaces because they
rendered it differently would be a third version of the truth".

## Goals / Non-Goals

**Goals:**

- A reader sees what a capture *is* without decoding the syntax they wrote.
- One rendering, supplied once, drawn everywhere.
- A list is usable where it is drawn.
- What was typed stays visible, so a misreading is catchable.

**Non-Goals:**

Editing structure from a row; hiding the captured text; inventing a descriptor
for an ordinary note; redesigning the timeline; changing the command line,
which §8.6 makes the debugging surface where raw text is the point.

## Decisions

### The summary carries the reading, not the ingredients

`NoteSummary` gains `kind`, `descriptor` and `items`. Not `fields` — a surface
handed raw fields would format them, which is the thing `descriptor.rs` refuses
on the grounds that two surfaces would then disagree invisibly.

`items` is on the summary rather than fetched per row. A timeline of a hundred
rows would otherwise make a hundred follow-up calls across the FFI boundary to
find out that ninety-eight of them are not lists.

### An ordinary note keeps rendering as its text

`Promoted::Note` renders empty, deliberately: a note's content is its text.
Surfaces treat an empty descriptor as "there is nothing structured to say" and
draw the text alone, which is what they do today. The change is additive — no
existing row gets worse.

### The list screen stays, and stops being the only way in

`lists` reaches a list through a button on the note page, because the tab row
was already full. That stays true for the *tag-wide* view, which gathers items
across several captures and genuinely is a separate surface.

What changes is that one capture's items are drawn on the capture. The button
was a workaround for rows that could not render themselves; rows that can
render themselves do not need it.

### Ticking from a row goes through the same call

The row calls `tick(tag, item, done)` — the operation the list screen already
uses. No second path, and no Kotlin deciding what completion means, which
`make android-decides` exists to prevent.

## Risks / Trade-offs

- **A row with many items is a tall row** → Long lists are drawn truncated with
  a count, and the tag-wide view is where a long list belongs. The threshold is
  a surface decision rather than a spec one.

- **Two renderings of "what this is" already exist in the wild** — the desktop
  formats some things itself → Those are replaced by the projection's
  descriptor, which may render slightly differently than a surface did. That is
  the point of the change, but it will look like a regression in any screenshot
  taken before it.

- **`NoteSummary` grows** → It crosses the FFI on every timeline read. The
  fields added are small, and `items` is empty for everything that is not a
  list. Worth measuring against §2.1's latency budget rather than assumed.

  **Measured, and the assumption was wrong.** The first version asked
  `list(tag)` per row — a whole-tag gather, including every task carrying the
  tag, to draw one row's items. A hundred rows with twenty-five lists cost
  66ms. `items_of(tgt)` reads only that capture's items and brings it to 16ms.
  The lesson is the ordinary one: the expensive part was not the bytes
  crossing the boundary, it was the query behind them.

- **Nothing forces a surface to use them** → The fields can be added and
  ignored, leaving the change half-done and invisible. The acceptance for this
  is a screenshot per surface, not a passing test.

## Open Questions

1. **How much of a long list belongs on a timeline row?** Three items and a
   count, or all of them? This wants looking at rather than deciding in
   advance.

2. **Should an unpromoted capture that *looks* like a list of things — plain
   prose with commas — be offered as one?** Tier 3's territory, and out of
   scope here, but the first thing someone will expect once lists render
   properly.
