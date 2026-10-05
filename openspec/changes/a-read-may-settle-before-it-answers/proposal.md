# a-read-may-settle-before-it-answers

## Why

Two artifacts disagree about `capture due`, and both are arguing from §8.8.

The spec: "This SHALL be done from the read-only handle: a read SHALL NOT take
the writer lock in order to discover or to resolve staleness (§8.8)."

The code takes it. `read::due` calls `promote::settle`, which is
`Session::open`, and the comment there says why — the alternative shipped:

> This used to be `let swept = Session::open(…); drop(swept)`, which threw the
> `Result` away. So a store somebody else was writing gave no sweep, no error
> and an answer from stale state — the one outcome this command must never
> produce, since "nothing is due" is exactly what the person is here to
> disbelieve.

So §8.8 is being read two ways. "Reads go direct, always" as *a read must not
block on a writer*, and as *a read must never answer from state it knows is
stale*. For `capture due` those pull opposite ways, because the whole question
is whether anything is due, and an unread capture may be.

The decision is that **the code is right and the spec moves**. What `due`
answers is worthless if it is quietly incomplete: "nothing is due" is the one
answer a person acts on by doing nothing.

## What Changes

- The requirement says a read **may** settle first where its answer depends on
  promotion, and that it reports rather than fails when it cannot.
- It carries the reason, so this is not re-argued from the sentence alone and
  "fixed" back to a read that cannot sweep.
- The distinction that does the work is stated: not taking the lock *to report*
  staleness, versus taking it *to remove* staleness. The first is the thing
  §8.8 forbids and the second is what `due` does.

## Capabilities

### Modified Capabilities

- `cli` — a read whose answer depends on promotion may settle before answering.

## Non-goals

- **No change to any command.** This is the document catching up with a
  decision already made and already recorded in an archived design note.
- **No weakening of §8.8 elsewhere.** Every other read still goes direct, still
  reports, and still must not block on a writer. `due` is the exception and the
  requirement names it as one.
- **No new behaviour when the lock is held.** `due` already reports that it
  could not take it and answers anyway.
