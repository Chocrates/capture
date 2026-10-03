## Context

`list_of` declines and says nothing about why. That is correct for a parser and
wrong for the person who wrote the capture: a grocery list sat in a real archive
for three days as a tagged note because its colon was missing, and the only
symptom was a list that did not appear.

The surface for this already exists. `suggestion_for` answers "the archive
thinks this might be X but was not sure", the note page draws it, and
`entity-pages` made it acceptable. A near miss is the same shape of statement.

## Goals / Non-Goals

**Goals.** Name the one missing character. Offer to insert it. Keep the guards
exactly as strict for *matching* as they are now.

**Non-goals.** Guessing in prose. Other near misses. Applying anything unasked.
Anything at capture time.

## Decisions

### The near miss lives beside `list_of`, sharing its guards

Written as a second function in `capture-parse`, not as a looser `list_of` with
a flag. Two reasons:

- A flag that makes a parser accept more is a parser with two meanings, and
  every caller then has to know which one it asked for. §3.1.5's guard is the
  valuable thing here and it must stay exactly as tight.
- The near miss asks a *different question*: not "is this a list" but "is this
  text one character away from being one". The answer is advice, and advice and
  recognition should not share a return type.

### Two misses, and the second is not an afterthought

An anchored tag with commas and no colon is the case from the archive. An
anchored tag with a colon and nothing after it is the other half: somebody who
typed `#groceries:` was writing a list and stopped, and today that is silently a
tagged note.

*Rejected:* a tag with no colon and no comma — `#reading finished the long one`.
That is an ordinary tagged note and always will be; suggesting a list there
would fire on most tagged notes ever written, which is §3.1.5's failure wearing
a different hat.

### Accepting writes an edit, and says so

Captured text is kept verbatim (§3.1.3), so the system does not insert the
colon. A person can, and `edit` is already how a person corrects their own
words.

This is a different act from accepting a promotion, which annotates and leaves
the text alone — so the surface says what will be inserted rather than offering
a bare "yes". Making the two look identical would teach somebody that accepting
a suggestion never touches what they wrote, which would then be false.

*Rejected:* a `list` promotion that carries items without the text saying so.
It would make the capture and its expansion disagree, which is the drift §3.1.4
spends its length keeping visible.

### It reuses `suggestion_for` rather than adding a second channel

One string, one surface, one gesture. A second "hints" channel beside
suggestions would double every surface's work to say the same kind of thing.

## Risks / Trade-offs

- **It will fire on tagged notes that happen to contain a comma.** `#reading
  finished the long one, finally` would be offered as a list. → A suggestion
  costs a glance (§3.1.3), it is never applied, and the alternative is the
  silence this change exists to remove. Worth measuring against real captures
  before deciding it is too eager.
- **Accepting rewrites captured text.** → It is an ordinary `edit`, authored by
  a person, visible in the note's history like any other, and the offer says so
  before it happens.
- **An edit re-parses the capture** (§3.1.4), which may change its type. → That
  is what making it a list *means*, and it is the same consequence any edit has.

## Migration Plan

None. No event kind, no body change, no projection column, no schema bump. A
suggestion is derived at read time and was never stored.

## Open Questions

- **Should the comma requirement be a count?** One comma is the minimum that
  distinguishes a list from a sentence; two would be stricter and would miss
  `#shop: milk, eggs`-shaped captures with exactly two items. Starting at one
  and looking at what it catches.
- **Does the offer belong in review as well as on the note page?** `review`
  already carries `suggestion`, so it arrives there for free. Whether a near
  miss is worth somebody's review slot is a question for a week of use.
