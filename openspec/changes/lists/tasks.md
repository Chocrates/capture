## 0. The one decision that blocks a surface

- [x] 0.1 Settled: `capture tick <list> <item>` and `capture untick`, a separate verb rather than overloading `done`. Original question: whether ticking from the command line is `capture done <list> <item>` or a separate verb. `done` already takes `<name>` and `<name> <occ>`, and telling them apart by whether the second argument parses as a date is the fragile disambiguation §3.1.5's guards exist to avoid. Amend `specs/cli/spec.md` to match whatever is decided, before §5 is written.

## 1. The form, in the parser

- [x] 1.1 Recognise a tag sigil, slug and colon anchored at the start of a capture, and read what follows as a comma-separated sequence. Tier 1: unscored, and it wins over any inference in the same line.
- [x] 1.2 The guards, which are the part worth getting right: a sigil anywhere but the start is an ordinary tag whatever punctuation follows, and nothing after the colon is not a list. `#standup at 9:30` and `ask sarah about #groceries: she has it` both stay prose.
- [x] 1.3 Trim items, discard empty ones between separators, and carry written position as display order.
- [x] 1.4 **Property test** rather than example test: for any capture text, recognising a list never panics, never changes the stored text, and either yields a tag with at least one non-empty item or yields nothing. The guards are where an anchored-match bug hides, and examples only cover the cases somebody thought of.
- [x] 1.5 Assert the vocabulary and sigil table are unchanged by this work — a test that enumerates both, so growing them stays the deliberate edit §3.1.5 requires.

## 2. Expansion, in the projection

- [x] 2.1 Expand a list capture's items into rows during the fold, following the occurrence machinery in `projection.rs`. Derived data: discarded and recomputed by a rebuild, never written to the log. No rolling horizon — a list's items are all known at parse time.
- [x] 2.2 Give an item an identity derived from its capture and position, used for display and never for completion state. §Decisions is explicit that binding completion to this would un-tick things when a list is edited.
- [x] 2.3 **Property test**: a rebuild reproduces the expansion exactly, for any set of list captures. This is the same property the occurrence expansion already asserts and the same reason — derived data that does not survive a rebuild is a silent corruption.
- [x] 2.4 Extend the tag query to gather expanded items alongside the tasks `mentioning` already returns, items first in display order and tasks after, newest first.

## 3. Ticking, in the log

- [x] 3.1 Complete an item by appending a promotion to a task carrying the item's text and the list's tag, then marking it done. Nothing is removed and the capture is not rewritten (§3.1.3).
- [x] 3.2 Match an expanded item to its completing task by `(tag, item text)`, so completion survives both a rebuild and an edit of the list capture.
- [x] 3.3 Deduplicate on that same match when reading a list, so a ticked item appears once rather than as both an item and the task that ticked it.
- [x] 3.4 Report an ambiguity where two items on one list carry the same text, and change neither. **Property test**: completing is idempotent — completing an item any number of times leaves exactly one task for it.
- [x] 3.5 Reopening: the same path backwards, leaving one task rather than a second.

## 4. Proving it before any surface exists

- [x] 4.1 A test that walks the whole thing against a real store: capture a list, read it, tick an item, rebuild the projection, read it again and find the tick still there. This is the first point at which the feature is real, and it should pass before a single line of UI is written.
- [x] 4.2 The retroactive case, which is the one most likely to surprise: a capture written before this change that matches the form becomes a list on the next rebuild, with its text untouched. Run it against a copy of a real archive, not a synthetic one.

## 5. The command line

- [x] 5.1 Read a list by tag, items in display order with their state, then the tasks carrying it. `--json` on the same terms as the other reading commands.
- [x] 5.2 Say so when a tag names nothing, rather than printing an empty result.
- [x] 5.3 Complete and reopen an item: `capture tick <list> <item>` and `capture untick`.
- [x] 5.4 Add it to `capture --help`, in the section the reading commands already live in.

## 6. The phone, which is the point

- [x] 6.1 Two FFI calls, operation-shaped per task 2.1 of M4: read a list, and tick an item. Not a general `mentioning()` and `task()` for Kotlin to compose — composing them there is Kotlin deciding something, which `make android-decides` exists to prevent.
- [x] 6.2 A list screen: items in order, tick-boxes, completed items visibly completed rather than gone.
- [x] 6.3 Reach it from a tag on a note page and from search. **Not a new tab** — the row already overflows on a real Pixel 8 Pro, wrapping "Timeline" mid-word and clipping "Health".
- [x] 6.4 Drive the whole loop on a physical device: capture a list from the home-screen widget, tick an item in the shop posture — one hand, screen at arm's length — and confirm what it recorded. This is the test the emulator cannot give.

## 7. The desktop

- [x] 7.1 The tag query answers lists in the window's existing surfaces. No new pane: the proposal makes a dedicated desktop list surface a non-goal.

## 8. Closing

- [x] 8.1 `make ci`, and the Android suite on a device.
- [ ] 8.2 Answer open question 3 with something learned rather than guessed: after using a real grocery list for a week, decide whether completed items should hide behind a toggle in the view. It is the first thing that will be noticed in real use and it should not be decided before then.
- [ ] 8.3 Record what the form got wrong in practice — what was typed that was expected to be a list and was not, and the reverse. The guards in 1.2 are the likeliest thing to be subtly wrong, and only real captures will show it.
