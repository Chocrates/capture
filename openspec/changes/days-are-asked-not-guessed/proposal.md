## Why

Found in use on 2026-10-13. "therapy next thursday at 4pm", written on a
Tuesday, became Thursday the 15th; the person meant the 22nd. They corrected the
text to "therapy Thursday 2026-10-22 at 4pm", which read as *the 15th again*,
all day: a weekday in front of a date was read as the weekday alone, and the
date and the 4pm after it were dropped. That reading was under the floor, so it
was offered as "This might be something on 2026-10-15". They accepted it, which
pinned an all-day entry on the wrong day that no later edit could move, and
nothing anywhere offered a way back.

Four faults, each of which alone would have been survivable:

1. **A weekday before a date discards the date.** `thursday oct 22`,
   `Thursday 2026-10-22` and `Thursday, 22 October` all read as "the coming
   Thursday", and the time after them is lost with the date.
2. **"next thursday" is guessed.** Said early in a week it means this week's to
   some people and next week's to others. The grammar picked one silently, which
   is the failure the bare hour was taken out of the grammar for (grammar 7).
   "last thursday" read as the *coming* Thursday.
3. **A suggestion hides what it leaves out.** "something on 2026-10-15" did not
   say *all day*, did not name the weekday, and did not say that the "4pm" in
   the text had been ignored — so accepting it looked like agreeing with what was
   written.
4. **Accepting a suggestion cannot be undone.** It is pinned, so it outlives
   every edit and every grammar, and no surface offers "read it again".

## What Changes

- **A weekday and a date that agree are one day.** `thursday oct 22 at 4pm`,
  `Thursday 2026-10-22 at 4pm`, `Thursday, 22 October 4pm` and `oct 22 thursday
  4pm` read as 22 Oct 16:00, at the band the date alone would have had.
- **A weekday and a date that disagree are asked.** "Thursday 2026-10-23" (a
  Friday) promotes nothing and records an *unclear day* with two choices: the
  Thursday nearest the date, and the date.
- **"next thursday" is asked when it could mean either week.** Where the coming
  Thursday is still in this week (Monday–Sunday), it records an unclear day: this
  week's or next week's. Where it is not, it is the coming one, as before. Said
  on a Thursday, it is a week away rather than today. "last thursday" is the
  most recent one before today.
- **An unclear day is shown where somebody looks for the appointment**: on both
  candidate days in the calendar (phone, desktop, `capture cal`), on the note
  page, and in review. Answering rewrites the day in the text as an ordinary edit
  ("next thursday" → "thursday oct 22"), and the capture promotes from that.
- **A suggestion says what it would write**: the weekday, "all day" where there
  is no time, and any time in the text it did not read.
- **A correction can be undone.** "Read it again" (phone, desktop, `capture
  reread NAME`) replaces a person's correction with what the current grammar
  reads, unpinned, so later edits take effect again.
- **A series keeps a written length** (`~1h`), already shipped in ca37939; noted
  here because it was found the same day and the projection version moves again.

## Capabilities

### Modified Capabilities

- `recognizer`: weekday-and-date, `next`/`last` + weekday, unclear day.
- `browse`: unclear days on the calendar and note page; suggestions name what
  they leave out; "read it again".
- `cli`: `capture day NAME 1|2`, `capture reread NAME`.

## Impact

- `crates/capture-parse`: `point()` reads weekday+date, determiners; new
  `UnclearDay`, `unclear_day(text, now, zone)`, `answer_day`; grammar 9.
- `crates/capture-time`: `FloatingDate::weekday`, `FloatingDate::plus_days`.
- `crates/capture-store`: projection table `unclear_days` derived from text on
  apply (schema 16); `calendar::days` returns them on both days;
  `mutate::settle_day`; `promote::reread`; `review::suggestion_for` wording.
- `crates/capture-ffi`, `android`, `apps/capture-desktop`, `apps/capture-cli`:
  the question and its answer, and "read it again".
