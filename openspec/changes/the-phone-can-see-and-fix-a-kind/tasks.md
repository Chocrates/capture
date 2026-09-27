## 1. The projection's answer

- [x] 1.1 A query returning each open task with its `due`, `done` and descriptor, against `promotions` where `kind = 'task'`. `done` filters in SQL; `promoted("task", n)` stays as it is, because the command line still wants a flat list.
- [x] 1.2 The band is decided in the store, not returned raw: overdue, due, undated, against a caller-supplied today in the reading zone. Two surfaces comparing dates themselves is two answers on the same day.
- [x] 1.3 **Property test**, not examples: for any set of tasks and any today, every open task lands in exactly one band, no done task lands in any, and a task with no due date always lands undated. A date comparison with a zone in it is where off-by-one lives, and three hand-written cases will agree with the bug.
- [x] 1.4 A test that an undated task is returned here and is absent from `agenda`. That absence is the whole reason this change exists, so it should fail loudly if someone later "helpfully" files undated tasks onto a day.

## 2. The binding

- [x] 2.1 `tasks()` on the FFI, returning the banded rows. Due dates cross as the floating `String` they are stored as — never an epoch millisecond.
- [x] 2.2 A uniffi enum for kind on the write path, so Kotlin cannot name a kind that does not exist. The existing `NoteSummary.kind: String` stays; it is a rendering and is not a mutation.
- [x] 2.3 `correct(name, as_)` wrapping `promote::correct`, writing `pinned: true, grammar: None, confidence: None`.
- [x] 2.4 `as_task(name, due)` wrapping `promote::as_task`, with `due` optional.
- [x] 2.5 A test through the binding — not the store — that a correction survives a later `sweep` under a newer grammar. The generator, JNA and the ABI are exactly what the Rust suite does not cover, and a pinned flag lost in translation looks correct on both sides.
- [x] 2.6 Regenerate the Kotlin binding and confirm it compiles before any screen is written.

## 3. The phone reads

- [x] 3.1 A task screen on its own route, banded overdue / due / undated, rendering the projection's descriptor rather than formatting fields itself.
- [x] 3.2 The undated band is present and labelled even when the others are empty. It is the ordinary case and the one that has been invisible.
- [x] 3.3 Marking a task done from the screen, through the existing `set_done`. No second path.
- [x] 3.4 An empty state that says nothing is outstanding, plainly. Not an error, not a congratulation — §0 says nothing scores the user.
- [x] 3.5 Reachable from wherever the phone's other surfaces are reached, so the timeline stops being the only way in.

## 4. The phone corrects

- [x] 4.1 The note page's below-floor suggestion line becomes actionable: accepting it writes a pinned correction. Needed a new `promote::accept`, because `suggestion_for` answers a sentence rather than a `Promoted` and `promoted_from` refuses below the floor by design — so the shape mapping was split out as `shape_as` and the floor check left where it was. Not a lowered floor: the floor governs what is written unasked.
- [x] 4.2 Changing a kind outright from the note page, including demotion to a plain note — a thing wrongly promoted is as common as a thing wrongly left alone.
- [x] 4.3 Setting or clearing a due date when naming something a task, as a floating date.
- [x] 4.4 No kind control on a timeline row. A mis-tap while scrolling would write a pinned correction that outlives every later grammar, and the note page is where the structured half already lives.
- [x] 4.5 The captured text is untouched by every path above. A test, because "promotion annotates, never rewrites" is the invariant a reclassify UI is most likely to break.

## 5. Seeing it

- [ ] 5.1 Against a store holding one of each kind — overdue, due today, undated, done, and a series — on a real device. Photographed before and after, as `surfaces-render-the-projection` 4.1 did; that task earned its keep by catching a race nothing failed on.
- [~] 5.2 **The property is proved; the on-device leg is not.** `two_devices.rs` now corrects a kind on one device, syncs over a real socket with real identities, and asserts the other agrees *and* that two further sweeps on the receiving side do not take it back — which is the failure worth catching, and it is deterministic. Driving the same path through an emulator and a real `capture` daemon is still outstanding. **Emulator or `dev.capture.dev` only** — never the real archive.
- [x] 5.3 Measured: **22.5ms** for 300 open tasks (100 overdue, 100 undated), debug build, as an ignored test rather than a CI threshold. Same order as the timeline's 16ms for 100 rows, which is what one indexed query against `promotions` should cost.
