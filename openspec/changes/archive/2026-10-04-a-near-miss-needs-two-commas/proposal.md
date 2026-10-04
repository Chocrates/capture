# a-near-miss-needs-two-commas

## Why

The spec says the first near miss is "an anchored tag with no colon, where **a
comma** appears after it". The code requires two, and its comment says why, with
evidence:

> One comma was the first rule and it was measured, which is the only reason
> this says two. Against hand-written tagged prose it fired on every single
> line: `#reading finished the long one, finally`, `#health slept badly,
> again`, `#journal a quiet day, mostly reading`. Six false offers out of eight.

So `#groceries milk, eggs` yields nothing, which the spec as written says it
should. The code is right: §3.1.5's whole argument is that a form firing inside
ordinary writing is worse than one that never fires, and this is that argument
with a measurement attached.

The spec is the normative artifact, so the two disagreeing is not a small thing
even when the code is the better of them. Somebody reading the spec and
"correcting" the code back to one comma would be following the document, and
would reintroduce six false offers in eight.

## What Changes

- The requirement says **at least two** commas, and carries the measurement as
  its reason, so the threshold is part of what is specified rather than an
  implementation detail somebody is free to tune.
- A scenario pins the one-comma case: `#health slept badly, again` reports no
  near miss. The existing scenarios all use three items, where one comma and two
  behave identically — which is exactly why the drift was invisible.

## Capabilities

### Modified Capabilities

- `recognizer` — the near-miss threshold is stated, and the prose case that set
  it is a scenario.

## Non-goals

- **No change to the code.** This is a change to the document, to match
  behaviour that was chosen deliberately and measured. If the threshold is ever
  revisited it should be revisited on new evidence, not because two artifacts
  disagreed and the code lost.
- **No new near-miss kinds.** "Two misses and no others" stands. Whether an
  unreadable `>` marker should produce a third is a question `the-sigil-names-
  the-day` recorded and deliberately left open.
