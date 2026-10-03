## Why

A capture that *nearly* matched a form is silent about it, and silence is the
worst available answer.

This one is real, from a real archive, three days before anybody noticed:

```
#groceries 7 apples, rye flour or bread, 500g pearl barley, 700g split peas,
1kg lentils, 3.2kg potatoes, 5 onions, 4 leeks, 1 cabbage, bag of carrots today
```

A grocery list for the day, written as one. It is not a list, and nothing said
so. The list form needs a colon after the tag — `#groceries:` — and without one
`list_of` declines and the capture stays a tagged note. No items, no tick boxes,
no error. The person who wrote it found out by asking why their list had not
rendered.

§3.1.5 is why the guard is strict, and it is right: a form that fires inside
ordinary writing is worse than one that does not fire at all, so the rule
demands an anchored tag, a single slug, and a colon. What §3.1.5 does not say is
that a *near miss* must be invisible. The guard exists to stop the system
*acting* on a guess. Saying "this looks like a list you meant" costs a glance,
which is the price §3.1.3 already sets for a suggestion.

The same silence covers the other direction. `#groceries:` with nothing after it
is a tagged note, deliberately — but somebody who typed the colon was writing a
list and stopped.

## What Changes

- **A capture that looks like a list without being one offers to be one.** Shown
  where below-floor recognitions are already shown: on the note page and in
  review.
- **The offer can be accepted**, which writes an ordinary `edit` inserting the
  colon. A person choosing it, never the system — §3.1.3 keeps captured text
  verbatim, and an edit is what a human correction already is.
- **Two near misses are recognised**, and only two:
  - an anchored tag with no colon, and a comma somewhere after it;
  - an anchored tag with a colon and nothing following it.
- **Nothing is applied automatically.** The floor's argument holds exactly: a
  wrong suggestion costs a glance and a wrong promotion costs a list somebody
  did not write.

## Capabilities

### New Capabilities

None. This is the existing suggestion surface answering one more question.

### Modified Capabilities

- `recognizer`: what a near miss is, and that recognising one SHALL NOT make it
  a match.
- `browse`: a near miss is offered where a below-floor recognition is offered,
  and accepting it is a person's edit.

## Impact

- `crates/capture-parse/src/lib.rs` — a function answering "this is not a list,
  and here is why it nearly was". Beside `list_of`, sharing its guards.
- `crates/capture-store/src/review.rs` — `suggestion_for` gains the case.
- `crates/capture-ffi`, `android/`, `apps/capture-cli`, `apps/capture-desktop` —
  the offer, and accepting it.

## Non-goals

- **Guessing where the colon goes in prose.** Only after an anchored tag, which
  is the one position the list form already cares about.
- **Any other near miss.** A date that nearly parsed, a recurrence that nearly
  did — each is its own argument about what "nearly" means, and this change
  takes the one with a real archive behind it.
- **Applying it without being asked.** That is the floor's whole argument, and
  it does not change because the guess feels obvious.
- **Warning at capture time.** §2.1 makes capture one gesture and zero
  decisions; a prompt there is the thing the design most consistently refuses.
