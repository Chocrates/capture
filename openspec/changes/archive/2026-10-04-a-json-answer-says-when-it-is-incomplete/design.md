# Design

## The shape has to change, and that has a precedent

`agenda --json` answers a bare array. Carrying a count means answering an object
instead:

```json
{"entries":[ ... ],"unread":3}
```

That breaks anything parsing it as an array. It is still right, and the argument
is already written in this file — `tasks --json` made exactly this change, and
its comment says why:

> An object rather than the bare array it used to be, so the count can ride
> along. A consumer reading an empty list as "no tasks" was being told something
> that might not be true.

The agenda has the same problem and a stronger case: a day with nothing on it is
a far more common answer than an empty task list, so the ambiguity is hit more
often. Making the two commands answer different shapes to avoid a break would
leave a worse inconsistency than the one being fixed.

`entries` rather than `agenda`: the field names what it holds, and the command
already names the rest.

## Why not put the count in a header line or stderr

Two alternatives that avoid the break, and both are worse.

Printing the warning to stderr while JSON goes to stdout keeps the shape and
loses the association — a consumer redirecting stdout gets clean JSON and never
sees the caveat, which is the same silence with extra steps.

A `--json` object only *when* something is unread is worse still: the shape
would depend on the state, so a consumer that worked all week breaks the first
morning the sweep falls behind. The field is always present, zero included, for
the same reason the prose path says nothing at zero but the schema still has a
place for it.

## Where the reporting goes

Beside the answer, not instead of it. `say_if_behind`'s own comment is the rule:
"printed after the answer, never instead of it. Suppressing the output would
make a read fail when the sweep is behind, which is §8.8's failure with the sign
flipped." The JSON path currently does neither — it does not fail and it does
not say — and this gives it the half it is missing without taking the half it
has right.

## What the test has to pin

That the field is present at zero, and that it carries a real count when the
sweep is behind. The second needs a capture that has not been read, which is an
ordinary `capture` with no `settle` after it — the same arrangement the prose
test would need and nothing exotic.

Worth noting why this went unnoticed: the requirement names three commands, and
only one of them was ever asserted. A scenario written against "these commands"
does not test anything on its own.
