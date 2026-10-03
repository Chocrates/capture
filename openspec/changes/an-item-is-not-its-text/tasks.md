## 1. The encoding

- [x] 1.1 `ItemBody` gains `ord` **and `of`** — the position, and how many items there were. The second was not in the plan and the rules are unsound without it: see 2.2.
- [x] 1.2 Kinds 28 and 29 unchanged. Both vectors pinned, and the compatibility property is visible in the bytes: with `ord` and `of` absent the trailing fields elide, so the encoding is **byte-identical** to what `ItemBody` produced before — every tick already written is still a valid encoding of this type.
- [x] 1.3 Decodes with `ord: None, of: None`, keeping its words.

## 2. Reconciliation in the fold

- [x] 2.1 **In the store, not the fold.** Matching a tick against a capture's items means expanding its text, `list_of` is in `capture-parse`, and that crate already depends on `capture-core` — the same cycle the entity mutes hit. The fold carries ticks raw in event order (`Note.item_ticks`); `write_into` reconciles them where it already expands the items.
- [x] 2.2 **The test found the rules were wrong.** Falling back to position could not tell a *reworded* item from a *deleted* one: remove the middle of three and the slot still exists, holding the item below it, which inherited a tick nobody gave it. Fixed by recording how many items there were and only comparing positions against the same count. The residual failure is now a tick that matches **nothing** rather than one that lands on the wrong row — losing is better than silently wrong.
- [x] 2.3 **Not `Held`, and the framing was wrong.** `Held` is for an event this build could not *understand* — malformed, or an unknown kind — and is re-examined after an upgrade. A tick matching no item is understood perfectly; it refers to something the capture does not currently say. So it is inert, and because reconciliation runs on every expansion, restoring the item restores the tick. Tested both ways.
- [x] 2.4 Asserted — and `of` improved the outcome. Rewording *and* inserting in one edit now leaves the tick matching **nothing** rather than landing on the wrong row. Losing one is visible and recoverable; being silently wrong tells somebody they already bought something they did not.

## 3. The projection

- [x] 3.1 Empty tag, no new table.
- [x] 3.2 Bumped, with its changelog entry: the ticks are in the log and were never in this table, so there is nothing to migrate — every list is simply re-expanded and re-reconciled.
- [x] 3.3 **Already existed** as `items_of(tgt)`, added by `surfaces-render-the-projection` so a row could draw itself. Nothing to build.
- [x] 3.4 A collection skips the captures whose items it is already showing, so a list promoted to a task reports **3 of 3** instead of 4 of 4. A task carrying the tag that is *not* one of those owners still belongs beside the items, which is what the union was for. An empty tag returns nothing: writing no tag meant no filing, and an empty name must not become the bucket every untagged list falls into.
- [x] 3.5 Both halves pinned: a *queued* redaction still expands its items, because withholding the text is a read-time `CASE` while the expansion reads the stored text; a *destroyed* one has none, which is the `purge_note` fix from the entity work doing its job.

## 4. Ticking

- [x] 4.1 `tick_at(name, ord, done)` added for surfaces, which know the row. `tick(tag, item, done)` stays for people typing words and now records the position it found, so a later edit cannot lose it. `Ambiguous` kept deliberately: it is still the right answer when somebody names two identical items by their words.
- [x] 4.2 By words it still refuses as `Ambiguous`, which is honest — nothing there can tell them apart. By position there is nothing to be ambiguous about, which is what a surface showing the list always knew and could never say.
- [x] 4.3 Asserted, and reproduced end to end through the command line: `2 of 3 left` stays `2 of 3 left` where it used to become `3 of 3`.

## 5. The recognizer

- [x] 5.1 `: milk, eggs, bread`. All three guards stand, and `#: milk` is still refused — that is the untagged form written with a sigil naming nothing, and reading it as a tag called "" would file things under nothing at all.
- [x] 5.2 Five sentences that a looser rule would have destroyed, including `note to self: milk, eggs` and `budget 40k: revisit in Q2`.
- [x] 5.3 **The existing suite caught it immediately.** Three tests failed on the new opener, two of them 2000-case property tests asserting invariants this change deliberately reverses — *a list always files under something* and *only an anchored sigil opens a list*. Both updated to the new rule with every other guard at full strength; the anchoring property still holds over arbitrary text.

## 6. The surfaces

- [x] 6.1 `tickAt(name, ord, done)`, and `ListLine` gains the capture it came from — a collection gathers items from however many captures wrote them, and a surface needs to know which in order to tick the right one. A capture's own checklist was already `NoteSummary.items`.
- [x] 6.2 Note page and list screen both tick by position. The list screen says "Everything tagged #x, from N captures" when N > 1 — this is a *collection*, not one person's list, and drawing two shopping trips as one is how a tag came to be treated as a list's identity.
- [x] 6.3 `capture tick-at <note> <n>` / `untick-at`, counting from 1 because that is how the list prints itself. `tick` by words stays for people typing them, and its ambiguous refusal now names the remedy instead of saying it cannot be done.
- [~] 6.4 **IPC added, nothing to wire it to.** `tick_item_at` is registered beside `tick_item`. The window draws items read-only on purpose — its own comment says completing one is a write and this row is a button that opens the note, so nesting a control inside it would make one gesture ambiguous — and `tick_item` has no caller in the UI either. Giving the desktop a ticking surface is new work, not a conversion, so it is not smuggled in here.

## 7. Seeing it

- [x] 7.1 **Run on a Pixel 8 Pro against `dev.capture.dev`, and it caught a bug.** The first attempt still answered "2 items on #shop read 'milk'… these cannot be told apart" — the *timeline row* renders items too and was still ticking by words. Converted along with every other caller, after which tapping the second of two identical rows ticks the second and leaves the first alone, which is a thing no amount of text matching can express. The untagged `: pack passport, charger, adapter` renders and ticks as well. Screenshots before and after.
- [x] 7.2 Measured at 50 captures, 500 items, 100 ticks: a capture's own checklist is **57µs**, the whole collection **3.1ms**. Nothing quadratic in the reconciliation, which was the part most likely to have been written that way. Kept as an ignored test rather than a CI threshold.

## 8. Found by driving a real device

- [x] 8.1 **Three of the five tick call sites were missed.** The note page and the list screen were converted; `NoteRow` — which draws items on the timeline *and* in search results — was not, so the surface most likely to be used was the one still matching by words. Found by the old refusal appearing on screen. Every caller now ticks by position, and `model.tick` has no caller left in the application.
