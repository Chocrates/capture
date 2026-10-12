## Context

The store already models what this change needs. A series is a rule plus
exceptions (§3.4); an occurrence is named `(series, original start)` and can be
moved, cancelled, amended and done by its own events (§3.5); moved occurrences
render at their new time and cancelled ones are hidden but recoverable (§4.4).
What is missing is the part a person touches: the grammar guesses an hour
wrong, no surface shows more than a day, and no surface but a raw command can
make an exception.

## Decisions

### D1. A bare hour is a question, not a time

`clock_after` accepts a `Number` after a `Marker` as `n * 60`. It will instead:

- `0`, `13`–`23`: a 24-hour time, as now. Nobody writes "at 17" meaning 5 am.
- `1`–`12`: no time. The recognition keeps its date or rule and its band, and
  carries `unclear_hour: Some(n)`.

Rejected: a waking-hours guess (1–6 pm, 7–11 am). Offered and declined by the
person, and the reason holds up — a guess that is right most days is the
failure that is only noticed when it matters, at 2 am, on the day of the
appointment. An untimed item that asks is never silently wrong.

The capture still promotes. Refusing to promote would leave "dentist on oct
15th at 2" as a plain note, off the calendar entirely, which is worse than an
all-day entry on the right day. A promoted event without a time is an all-day
event; a series without a time is an untimed series. Both already exist.

The *offer* reuses the near-miss path (browse: "A near miss is offered where a
suggestion is offered"): it names what will be inserted, accepting writes an
ordinary edit, and declining changes nothing. It generalises from one fix to a
choice of two (`am`, `pm`), which the list near miss never needed.
`capture_parse::unclear_hour(text)` returns the byte offset after the number
and the hour, so the edit inserts exactly `pm` there and nothing else moves.

Grammar version bumps, so every capture re-promotes once and existing 02:00
series become untimed with a question — the right outcome for each, since
none of them were ever meant to be at 2 am.

### D2. One calendar read

`projection::calendar(from_day, to_day, zone) -> Vec<CalendarDay>` where each
day carries all-day items, timed items (start, end, slug, occurrence key,
`moved_from`, done, skipped) and tasks due that day. The one-day agenda becomes
`calendar(day, day)` underneath, so the two cannot disagree about a day.

Skipped occurrences are *returned*, flagged, rather than filtered out. §4.4
says "hidden but recoverable", and recoverable needs a place to press Restore.
The day list shows them dimmed with Restore; the month grid does not count
them as busy.

Range is clamped to the occurrence horizon (§3.4, ±1 year). Beyond it the read
says so rather than returning an empty month that looks like a free one.

### D3. Month grid and week strip, one screen

The person asked for both. Both sit above the existing Agenda bands, toggled in
place; the selected day is shared, so switching shapes keeps the day. Swipe
moves by the shape's unit. A day cell marks busyness with up to three dots
(timed, all-day, task) rather than titles: a phone cell is too small for text
and a truncated title reads as a different appointment.

### D4. An occurrence is named by date where people name it

The CLI takes `NAME DATE` (and `NAME DATE TIME` when a series occurs more than
once a day, which no current rule produces). It resolves to the occurrence's
original start through the occurrences table, and refuses with the candidates
when the date names none or several. Epoch milliseconds stay accepted, for
scripts and for the existing tests.

### D5. Moving takes text, and keeps the time of day by default

`--to "thursday 10am"` is read by the recognizer as a point. A day with no time
("thursday") keeps the occurrence's original time of day, because "move it to
Thursday" means the same slot on another day. A bare hour is refused with the
same question as D1 rather than guessed. The resolved instant is shown before
writing: on the phone as a line under the field that updates as you type, in
the CLI as the confirmation it prints.

The duration is kept: a moved occurrence ends as long after its new start as it
did after its old one.

### D6. "Change this and later" is the existing split

`promote::split_series` already implements §3.5's this-and-future. The FFI
gains it and the phone's sheet offers it; nothing about its semantics changes.

## Risks

- Existing tests assert "every weekday at 7" is 07:00. They change to assert
  the question. That is the point of the change, not collateral.
- A person who always means 7 am now answers a question once per capture. The
  answer is one tap, and writing "7am" avoids it entirely; the capture window's
  hint line already shows `friday 2pm`.
