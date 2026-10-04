## 1. The projection's answer

- [x] 1.1 `NoteSummary` gains `kind`, `descriptor` and `items`. Filled where a store is open — the timeline read and the note read — rather than recomputed per surface, so two surfaces cannot render one note differently.
- [x] 1.2 An ordinary note answers an empty descriptor and no items, which is what `Promoted::Note` already renders. Additive: no existing row changes.
- [x] 1.3 A test that the same note answers the same rendering through the timeline and through the note read. They are separate queries and the whole argument for putting the rendering in the projection is that they cannot diverge.

## 2. The phone

- [x] 2.1 Timeline rows render the promotion: a task with its state and due date, an event with its time, a series with its rule. The captured line stays on the row, secondary — §3.1.4's drift has to stay catchable.
- [x] 2.2 A list capture renders its items with tick-boxes, completable in place through the same `tick` call the list screen uses. No second path.
- [x] 2.3 The note page shows the structured half beside the text, which `browse` has required all along and no phone surface has ever done.
- [x] 2.4 Drop the "Open the #groceries list" button from the note page. It was a workaround for a row that could not render itself. The tag-wide list screen stays and is reached from a tag.

## 3. The desktop

- [x] 3.1 The same, through the existing IPC. Where the window formats fields itself today, it takes the projection's descriptor instead — a second rendering is a third version of the truth.

## 4. Seeing it

- [~] 4.1 **Phone done, desktop outstanding.** A day holding all five was captured on a Pixel 8 Pro and photographed before and after. It earned its keep immediately: the first shot showed the list rendering and the task, event and series showing *nothing*, because `onResume` fired `settle()` and the screen's own read as two coroutines and the read usually won — so a capture made moments earlier drew as plain text until somebody navigated away and back. Nothing failed, which is why only looking would have found it. `settle()` now re-reads the current screen when it finishes. The desktop half needs a person: the window runs, but this compositor refuses programmatic capture every way there is — `grim` wants wlr-screencopy, which GNOME does not implement; X11 `import` cannot grab a Wayland root; and `org.gnome.Shell.Screenshot` answers `AccessDenied`. It is one PrtSc for somebody sitting at the machine, against a store holding one of each kind.
- [x] 4.2 Measured, and the claim was wrong. A hundred rows with twenty-five lists took **66ms** per read: `read_as` asked `list(tag)` per row, which gathers every capture under the tag *and* every task carrying it — a full tag query per row to draw one row's items. Replaced with `items_of(tgt)`, one indexed query against `list_items`: **16ms**. Both figures are a debug build; the shape of the difference is the point, not the absolute. The measurement lives as an ignored test (`--ignored --nocapture`) rather than an assertion, because a threshold in CI measures whatever else the machine was doing.
- [x] 4.3 Answered by looking. Uncapped, a twelve-item shop took **1200 of the Pixel 8 Pro's 2244 pixels** — one capture, most of a screen, the rest of the day below the fold. Capped at five with "and N more"; the same screen then holds eight captures. Five rather than three because four items is an ordinary short list and truncating one of those would be noise. The note page still draws all of them, which is where a list somebody is working through belongs.
