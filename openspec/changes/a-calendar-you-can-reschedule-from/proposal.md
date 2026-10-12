## Why

Found in use on 2026-10-12. Three things a person reaches for in the first
week, and none of them works end to end:

1. **"gym every monday at 2" schedules the gym for 02:00.** So does "dentist on
   oct 15th at 2". `clock_after` reads a bare number after a marker as a
   24-hour hour, so every afternoon appointment typed the way people type them
   lands in the middle of the night, and nothing says so. DESIGN §3.1.5 used
   "every day at 7 → 07:00" as its example, and that example is exactly the
   guess that goes wrong for the other half of the clock.
2. **There is no calendar.** The phone's agenda shows today and cannot move
   off it. The desktop shows one day with previous and next. Nothing anywhere
   shows a week or a month, so "what does next week look like" and "when is
   the dentist" have no answer short of searching.
3. **An exception cannot be made where anyone would make it.** The store has
   had per-occurrence move, skip, amend and done since 2026-10-04 (§3.5), but
   the only way to reach move or skip is `capture move NAME OCC START_MS`, with
   both instants in epoch milliseconds. The phone exposes none of them; the
   desktop only ticks. "I was sick, so this Monday's gym is Thursday at 10,
   just once" is the ordinary case §3.5 was designed for, and it is
   unreachable.

## What Changes

- **An hour without am or pm is asked, not guessed.** After a marker, a bare
  `1`–`12` no longer becomes a time. The capture still promotes — to the date,
  or the rule, without a time of day — and records an *unclear hour*: an offer
  to insert `am` or `pm` after the number, shown wherever near misses are
  (review, the note page, the phone's banner). Accepting is an ordinary edit,
  so the promotion follows the text. `0` and `13`–`23` stay unambiguous 24-hour
  hours; `2pm`, `14:00`, `noon` and time-of-day words are unaffected.
  The person chose this over a waking-hours guess.
- **A calendar, in month and week shapes, on every surface.**
  - A store read, `calendar(from, to)`, returns each day in a range with what
    happens on it: all-day items, timed occurrences (with their original start
    when moved), and dated tasks. One read, so every surface agrees.
  - **Phone:** the Agenda screen gains a month grid and a week strip, switched
    in place, with the chosen day's existing bands below. Swiping moves by
    month or week; Today returns. Busy days are marked.
  - **Desktop:** the same month grid beside the existing day view.
  - **CLI:** `capture cal [YYYY-MM | --week [DATE]] [--json]`.
- **One occurrence can be changed from wherever it is shown.**
  - Tapping an occurrence offers **Done**, **Move this one**, **Skip this one**
    and **Change this and later**. Move takes ordinary text ("thursday 10am"),
    shows the resolved time before writing, and keeps the original time of day
    when only a day is given. A moved occurrence is drawn at its new time,
    labelled with where it came from. A skipped one is drawn as skipped on its
    day, with Restore.
  - The CLI names an occurrence by its date (`capture move gym 2026-10-12 --to
    "thursday 10am"`, `capture skip gym 2026-10-12`); epoch milliseconds remain
    accepted.
  - The FFI gains the occurrence operations and the series split, which it has
    never had.

## Capabilities

### Modified Capabilities

- `recognizer`: a bare hour after a marker is no longer a time.
- `browse`: a calendar exists, in two shapes, and occurrence exceptions are
  reachable from it on the phone and the desktop. A skipped occurrence is shown
  as skipped rather than hidden, so Restore has somewhere to be.
- `recurrence`: a moved occurrence is shown with its origin.
- `cli`: `capture cal`, and occurrences named by date.

## Impact

- `crates/capture-parse`: `clock_after`, a new `unclear_hour`; the near-miss
  type generalises to more than lists. Existing tests that assert "at 7" is
  07:00 change, deliberately.
- `crates/capture-store`: `projection::calendar`, occurrence lookup by date,
  `mutate::settle_hour`, and a "move to text" resolver over the recognizer.
- `crates/capture-ffi`: calendar, occurrence operations, split, unclear hour.
- `android`: Agenda screen (grid, strip, navigation, occurrence sheet).
- `apps/capture-desktop`: month grid, occurrence actions.
- `apps/capture-cli`: `cal`, date-named occurrences, `hour NAME am|pm`.
- `DESIGN.md` §3.1.5's "at 7" example is corrected.
- Archives already holding a 02:00 series re-promote on upgrade, because the
  grammar version changes; nothing is rewritten in the log.
