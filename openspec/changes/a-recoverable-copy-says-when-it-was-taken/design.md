# Design

## Why a pure function, and why that is the point

The obvious shape is two more arguments inside the loop. Instead the line
becomes a function taking a snapshot, a clock and a zone, and returning a
string, with a unit test beside it.

Not for tidiness. It is the only way this can be tested at all.

Producing a snapshot means a redaction that has actually been carried out. The
cancellation window is thirty minutes of real time (§1.1), the CLI has no clock
to inject, and nothing drains the queue early — so a test at the command-line
level would have to wait half an hour. That is why `capture recover` is invoked
by no test in the repo, and why a field could be dropped on the floor at the
surface while the store test below it asserted the field was there.

A pure function moves the part that was wrong into reach. It does not test that
`recover` calls it, which is a real gap and an honest one: the alternative is a
clock injection for the whole binary, which is a much larger decision than this
change should be making.

## The date, not the instant

Prose gets a date in the reading zone. `--json` gets the raw milliseconds.

A date is what somebody remembers — "the thing I destroyed on Tuesday" — and a
timestamp to the millisecond in a listing is noise that makes the first line
harder to find. A consumer, conversely, cannot do anything with a rendered date
and can do everything with a number, and `expires_in_ms` beside it is already a
number.

## What stays wrong on purpose

The listing still prints only the first line of a destroyed note. That is
deliberate and worth restating while changing the line it sits on: the purpose
of the listing is to recognise a copy, and printing more would make the listing
a second place the content lives — which is the thing §1.2's retention is
already an uncomfortable exception to.
