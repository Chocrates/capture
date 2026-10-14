## Context

Grammar 7 established that an ambiguity the person can settle in one tap is
asked rather than guessed (the bare hour). This change applies the same rule to
days, and repairs the two paths by which a misreading became permanent.

## Decisions

### D1. Weekday and date: agreement is confirmation, disagreement is a question

A weekday adjacent to a complete date (either order, a comma allowed between)
is read as one anchor. When the weekday is the date's weekday the date is the
answer and the weekday is redundant confirmation, so the band is the date's.
When they disagree there is no basis for preferring either — people mistype
both — so nothing is promoted and an unclear day offers the Thursday nearest the
date (within three days either side) and the date itself.

### D2. "next X" is ambiguous only inside the current week

Weeks start on Monday, as the grid does. Let *c* be the first X strictly after
today. If *c* falls in the current week, "next X" could be *c* or *c*+7: asked.
Otherwise *c* is next week's X under either reading: answered. Said on X itself,
*c* is a week away, which both readings agree on. "this/coming/upcoming X" keep
on-or-after (today counts). "last X" is the most recent X strictly before today.

### D3. Answering rewrites the words, not a field

The answer replaces the span the recognizer flagged ("next thursday", or
"Thursday 2026-10-23") with `<weekday> <mon> <day>` — e.g. "thursday oct 22" —
which D1 reads back as exactly that day. The ISO form is used instead where the
month-and-day form would resolve to a different year from the capture's moment.
It is an ordinary edit, so the text says what the calendar shows (§3.1.4), and
the promotion follows the text through the sweep, as the hour answer does.

### D4. Unclear days live in the projection, derived from text on apply

The calendar has to show the question on the days it concerns, and the note has
no promotion to put it on a day. The projection already derives mentions and
list items from text as it applies events; unclear days join them in a table
`unclear_days(tgt, first, second)`, computed from the note's text, capture
moment and zone. Every device computes it identically, it needs no new event
kind, and a rebuild recomputes it. A grammar bump that changes it bumps the
projection schema with it. A note whose promotion is pinned is not asked about:
somebody already answered.

### D5. Suggestions say what they leave out

The wording names the weekday ("Thu 15 Oct"), says "all day" for a date without
a time, and names a clock in the text that the reading did not use. Not a new
band and not a refusal: the person still decides, with the information to.

### D6. "Read it again" writes an unpinned promotion

`promote::reread` recognizes the text against its capture moment and writes the
result unpinned with the current grammar — what the sweep would have written
had there been no correction — or `note` where nothing temporal is read. The
correction stays in the log; it is simply no longer the latest promotion. This
is an undo of a *pin*, not of history.

## Risks

- A capture that says "next friday" now waits for an answer instead of landing
  somewhere. That is the point; the question is on the calendar, both days.
- Grammar 9 re-reads every capture once; notes that read "next X" inside the
  current week lose their date until answered. Only the person's own new
  captures are affected in practice (the real archive has four dated notes).
