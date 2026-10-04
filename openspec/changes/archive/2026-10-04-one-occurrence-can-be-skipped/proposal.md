## Why

The browse spec describes a state nothing can produce:

> **Scenario: A cancelled occurrence is hidden but not gone**
> - **WHEN** an occurrence has been cancelled
> - **THEN** …it remains recoverable because the cancellation is an ordinary event

Nothing cancels one. `OCC_CANCEL` (21), `OCC_MOVE` (19) and `OCC_EDIT` (20) have
a frozen kind number, a documented body, a fold arm, a projection column, a
conflict rule, a calendar that hides cancelled occurrences, a log label and
tests — and **no emitter on any surface**. The only way to reach the state is a
hand-written `s.append(Kind::OCC_MOVE, …)` inside a test
(`tests/occurrences.rs:178`).

Found by auditing every `Kind` for an emitter, after two smaller versions of the
same shape: `mute_entity` was public, tested and unreachable, and the held-event
count had no way to list what it counted.

The recurrence spec is explicit that these events exist and what they mean:

> `occ_move`, `occ_edit`, `occ_cancel`, `occ_done` and `occ_undone` SHALL carry
> `tgt = series_id` and `occ = original_start`, and SHALL affect only that
> occurrence.

Two of the five are reachable. `capture done <series> <occ>` writes `OCC_DONE`
and `OCC_UNDONE`. The other three were built in the same change (m2 task 2.1
numbered all five together, 3.4 implemented the cancel-versus-move conflict) and
only two were wired up.

The cost is an ordinary thing somebody cannot do: **skip one standup.** The
series is right, the rule is right, and this Thursday is not happening. Today
the options are to edit the rule — which changes every future occurrence — or to
leave a calendar entry standing for a meeting that was cancelled.

## What Changes

- **`capture skip <series> <occ>`** writes `occ_cancel`. One occurrence, the
  rule untouched.
- **`capture unskip <series> <occ>`** writes `occ_move` back to the occurrence's
  own original start. That is not a workaround: §6 and m2's task 3.4 make a move
  beat a cancel deliberately, because *"a resurrected occurrence is visible and
  one gesture away; a wrongly cancelled one is invisible and silently missed."*
  There is no un-cancel event and there should not be.
- **The occurrence is named the way `capture done` already names one** — its
  original start, as the series' other two exceptions take it. One convention,
  not a second.
- **Cancelling an occurrence that has already been cancelled is not an error.**
  It is the state somebody asked for.

## Capabilities

### Modified Capabilities

- `cli`: that one occurrence of a series can be skipped and restored.

## Impact

- `crates/capture-store/src/mutate.rs` — two functions beside `occurrence`.
- `apps/capture-cli/src/args.rs`, `write.rs`, `main.rs` — the commands.

## Non-goals

- **Moving an occurrence to a *new* time.** `occ_move` carries it and the fold
  applies it, but a usable surface needs a time somebody can type, and the
  existing occurrence commands take milliseconds because they were written as a
  test harness (§8.6). Choosing how a person writes "this Thursday's one, at
  4pm" is a separate argument from making the exception reachable at all, and
  bundling it would hold up the half that needs no new decisions.
- **`occ_edit`.** A different title, location or notes for one occurrence. Same
  reason — it needs three optional arguments and a view that shows them — and
  it is the least reached-for of the three.
- **Doing this on the phone or in the window.** §8.4 wants everything reachable
  from the desktop app and this is a step toward that, not the end of it. The
  command line first, because it is where the state can be produced and tested.
- **A new event kind.** All three exist, are numbered, and are frozen. This
  change writes no encoding.
