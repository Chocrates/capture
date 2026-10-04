# Design

## Nothing new underneath

`tasks_outstanding` already assigns the bands and already has two callers. This
adds a third. The query is not touched: one implementation read by three
surfaces is what keeps them agreeing, and its boundaries — a task due *today* is
not overdue — are property-tested rather than example-tested, because a handful
of hand-picked dates agrees with an off-by-one as readily as with the fix.

The command mirrors the phone's binding field for field, including the band as a
value rather than as three separate lists. A surface that received three lists
could not say "nothing outstanding" without checking all three, and would be
free to draw them in a different order.

## Completing through the same write

`promote::set_done` — the one `capture done` makes and the one the phone makes.
The window already exposes `set_task_done`, so this needs no new command for
completion; the Tasks view calls what is there.

That is worth stating because the alternative is quietly attractive: the view
has the `tgt` to hand and could write the event itself. Two paths to one state
is how two surfaces come to disagree, and §3.6's completion rules (a task has
state, a series has state per instance) are exactly the kind of thing a second
path gets subtly wrong.

## Withholding was checked rather than assumed

`tasks_outstanding` filters on the promotion's kind and whether it is done, and
on nothing else — no `muted_until`, no `redact_queue`. So a task whose
destruction has been *ordered* stays in the bands for the cancellation window,
which looked like the leak this session has already found three times.

It is not. The row carries `descriptor`, composed from the promotion's kind and
due date, and the surfaces take the words separately from `text_of`, which
applies the same withholding `NOTE_COLUMNS` does. So a queued task draws as
"something is owed, and when", with nothing the redaction was asked to remove —
and the promotion row goes when the purge runs.

Asserted rather than left as a reading: `a_task_whose_destruction_is_ordered_is_still_outstanding`
pins both halves, so a `descriptor` that ever starts carrying the capture's words
fails a test instead of shipping.

## Where it sits

Beside Agenda in the nav, because the two answer the halves of one question —
the agenda is what occupies time, and this is what is owed. §3.6's whole
discriminator is that these are different things, and putting them next to each
other is the arrangement that teaches it.
