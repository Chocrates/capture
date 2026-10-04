# the-sigil-names-the-day

## Why

The published grammar promises `>` followed by either a full date **or** a day
reference — `sigil = "#" slug | "@" slug | ">" ( iso_date | day_ref )` — and
`day_ref` has no implementation anywhere in the repo. A `>` marker that is not
exactly ten bytes of `YYYY-MM-DD` is recorded in the recognition and then
discarded, after which the inferred date from the prose is promoted instead.

So `dentist thursday 2pm >fri` promotes **Thursday**, at confidence 85, with
`markers.date = Some("fri")` sitting in the same recognition. That inverts the
recognizer's first rule — an explicit marker beats inferred text (§3.1.1) — at
exactly the moment somebody wrote the marker *to correct* the prose, and it is
silent. DESIGN §3.1.5 says a match firing in prose is worse than no match; this
is that case, produced by throwing away the user's own correction. `>fri` is the
literal example in the spec's own first scenario.

## What Changes

- `>` accepts a day reference as well as a full date: a weekday from the closed
  `WEEKDAYS` vocabulary (`>fri`, `>friday`), and a day word from `TIME_WORDS`'
  neighbours (`>today`, `>tomorrow`). Resolution goes through the **same**
  `next_weekday` and `day_word` used by prose, so the meaning of "friday" is one
  decision rather than two that can drift.
- A day reference behind a sigil carries the same `confidence: 100` an ISO date
  does, and the same task/event discrimination from the head verb.
- **BREAKING (behaviour, not format):** a `>` marker that names nothing readable
  no longer lets the prose quietly supply a date. The capture keeps its text
  verbatim and is promoted with no date at all. An explicit instruction that
  could not be followed must not be replaced by a guess — that is the whole of
  §3.1.5 — and the alternative on the table, silently using the prose date, is
  the defect this change exists to remove.
- The `>satellite` trap is refused: matching goes through the closed vocabulary,
  not `weekday_bit`'s three-letter `starts_with`, which would read "satellite"
  as Saturday and break the closed-vocabulary requirement.
- Tests pin the negative halves, which is where this defect lived: no sigil-plus-
  prose disagreement was tested in any spelling, and the strings `>fri`,
  `>friday` and `>tomorrow` appear nowhere in the repo.

## Capabilities

### Modified Capabilities

- `recognizer` — the sigil tier now reads a day reference, states what an
  unreadable marker does, and keeps the closed vocabulary closed.

## Non-goals

- **No new date vocabulary.** `>fri` and `>tomorrow` work because prose already
  reads "fri" and "tomorrow". Nothing is added to `WEEKDAYS` or the day words,
  and `>q2`, `>eow`, `>next week` stay unreadable. Widening the vocabulary is a
  deliberate edit (§3.1.5) and not this change.
- **No clock behind the sigil.** The marker names a day and nothing else. A
  prose clock is not combined with it, which is already how an ISO marker
  behaves — `dentist 2pm >2026-09-20` is a date, not a moment — and this change
  leaves that alone rather than making a day reference the one spelling that
  picks up the time.
- **No near-miss for an unreadable marker.** Telling somebody at review time
  that `>asdf` could not be read is the right follow-on, and it needs the
  near-miss surface to carry a second kind of miss. Recorded as an open question
  in `design.md` rather than guessed at here.
- **No change to `CONFIDENCE_FLOOR`.** This change raises nothing's confidence
  except where a date was explicitly written, which is the one case the floor
  was never about.
