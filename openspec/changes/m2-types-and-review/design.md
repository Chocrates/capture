## Context

M1 is complete and in use on one machine: capture, store, fold, search, a GUI
covering the timeline, the note page and every mutation. Every capture is text,
and nothing reads it.

Two things exist that this milestone builds directly on. `brain-parse` holds the
recognizer's vocabulary, grammar and confidence bands (`DESIGN.md` §3.1.5) as
tested tables with no parser over them. `brain-core` holds a frozen encoding, a
deterministic fold, and event kinds up to `UNFLAG(15)`.

Two constraints shape everything below. The encoding is **permanent from its
first write**, so every new body is a decision that cannot be revised once an
event carrying it exists. And the fold takes the event set and nothing else
(`DESIGN.md` §8.3) — no clock, no locale, no local state — which is what makes
convergence testable by property rather than by hand.

## Goals / Non-Goals

**Goals:**

- A capture that says "dentist thursday 2pm" becomes a timed event, without the
  captured text changing.
- Recurring series stored as rules, expanded lazily, with per-occurrence
  exceptions that do not touch the series.
- Scheduled review, local intents and activation — the components with evidence
  behind them — shipped here rather than last.
- Calendar and agenda views.
- The capture gesture unchanged, deliberately (`DESIGN.md` §8.10).

**Non-Goals:**

- Sync (M3), Android (M4), enrichment (M5), the tier-3 model fallback.
- Any change to the chord, the overlay or the commit key.
- Entities on the note page — still nothing to populate them.

## Decisions

### `brain-time` is a separate crate, `no_std`, and cannot read a clock

Recurrence expansion is the one place in this design where a wrong answer is
invisible: an occurrence at the wrong instant looks exactly like an occurrence.
Putting expansion in a crate that cannot reach a clock makes "the same rule
expands identically everywhere" a property the type system enforces rather than
a convention.

*Alternative considered:* expansion inside `brain-store`, where the clock already
lives. Rejected — it would make the expansion untestable by the convergence
property that already exists for the fold, and §8.3 records the two earlier
violations of exactly this rule that the property test could not catch because
both were specified as intended behaviour.

**Consequence:** every entry point takes `now: u64` as a parameter. This is
mildly tedious and is the point.

### The recognizer returns a candidate, not a promotion

The parser produces a *recognition* — shape, fields, confidence — and nothing
else. Deciding whether it clears the floor, whether a human pin already governs,
and whether to author a `promote` happens above it, where the event log is
reachable.

This keeps the parser a pure function of `(text, now, timezone)` and therefore
exhaustively testable over the closed vocabulary. It also means the confidence
floor is enforced in one place rather than at every call site.

*Alternative considered:* the parser emits `promote` events directly. Rejected —
it puts event authorship inside the component most likely to be rewritten as the
grammar improves, and §3.1.3's guarantee that promotion is additive would then be
the parser's responsibility rather than a property of the caller.

### Occurrence identity is `(series_id, original_start)`, and exceptions are events

Following iCalendar `RECURRENCE-ID`. The value of this is that it names an
occurrence that has never been materialised — completing next Tuesday's gym
session requires no row for it to exist first, which matters because occurrences
are projection data that a rebuild discards.

*Alternative considered:* materialising occurrences as events. Rejected in the
design already — a daily habit would write 365 events a year into a log that
syncs to a phone, and the log is meant to be worth reading in ten years.

### "This and future" splits the series rather than editing the rule

Bound the existing rule with `UNTIL` immediately before the occurrence, start a
new series at it, link by `split_from`. Editing in place would silently rewrite
what the schedule *was* last March.

That matters more here than in an ordinary calendar: this system's purpose is
compensating for unreliable memory, and a log that quietly revises history is an
unreliable narrator in exactly the way that defeats the purpose.

### Promotion conflicts resolve by authority before recency

A human `promote` carries `pinned: true` and beats a parser-authored one
regardless of HLC. This is the one conflict rule in the system that is not plain
last-write-wins, and it exists because the parser *will* improve and reparse the
backlog — under plain LWW, a later machine parse would silently revert a
correction the user already made.

Losing trust that way is unrecoverable, which is a stronger reason than the
consistency of having one rule everywhere.

### Review records engagement, never accuracy

`shown / engaged / deferred`, and no score, streak or aggregate anywhere. The
scheduler works from engagement, so the graded outcome a spaced-repetition
schedule would normally need is not merely unnecessary — it is the thing most
likely to do harm, on a permanent immutable log, to a user whose stated
difficulty is memory.

*Alternative considered:* `recalled / missed / snoozed`, which an earlier draft
specified. Withdrawn in `DESIGN.md` §2.7 and not revisited here.

### The due queue grows rather than a scheduler being added

Review sessions and time-based intents join redaction deadlines in the existing
due-work queue, processed at every process startup with a resident window making
deadlines land near their time. No timer thread, no daemon — the property that
matters is that nothing is silently not running.

## Risks / Trade-offs

- **The encoding is frozen and these bodies are new.** A field forgotten in
  `promote` or `occ_move` is permanent. → Golden vectors for every new kind
  before any code writes one, and the round-trip property test extended to cover
  them.

- **Recurrence is where correctness bugs hide.** DST, unbounded series, horizon
  exhaustion, splits interacting with exceptions. → Property tests over expansion
  (expansion is deterministic; a rebuild reproduces it exactly), and the DST
  cases from §3.2 as explicit fixtures rather than as reasoning.

- **The vocabulary is unproven.** §3.1.5 was written without a parser to test it
  against; "covers the large majority of real input" is an assertion. → The
  parser's test suite is exhaustive over the closed vocabulary, and the
  suggestion tier means a miss costs a glance rather than a wrong promotion. Real
  captures during M1 use are the only honest sample, and they exist.

- **False positives are the failure mode that matters** (§3.1.3), and the guards
  are not part of the grammar. → They are specified as requirements with the
  document's own examples ("the march timeline slipped", "the 7am problem") as
  scenarios, so a grammar change that quietly drops one fails a test.

- **This milestone is large enough to displace the trial it exists to enable.**
  §8.10 says M1 should be used while M2 is built, and the 90-day observation
  window does not wait for M2. → The capture gesture is frozen; nothing here
  requires pausing use of M1.

- **`carries_forward` defaults to false, and the class of thing that needs it is
  exactly the class where forgetting hurts most** — a missed dose, an unpaid
  invoice. → The flag is never inferred, and a repeatedly-lapsing series prompts
  once in review rather than the system guessing.

## Migration Plan

No data migration. Every new capability is additive: existing notes are
unpromoted, existing projections rebuild into the widened schema, and a build
without these kinds holds them rather than misreading them.

Order of work, each step usable before the next:

1. `brain-time` — instants, floating dates, RRULE subset, DST policy. No
   dependants yet.
2. New event kinds and bodies in `brain-core`, with golden vectors. Fold rules
   and conflict rules with property tests. Nothing authors them yet.
3. The parser in `brain-parse` over the existing tables, returning candidates.
4. Promotion: authoring `promote`, the reparse path, pinning.
5. Projection: occurrence expansion, the widened due queue.
6. Surfaces: agenda and calendar in the desktop, then the CLI equivalents.
7. Review, intents, activation — each depends on the due queue from step 5.

Rollback is per-step: nothing before step 4 writes an event, and steps 1–3 are
inert additions.

## Open Questions

- **What the review session's fixed size actually is.** §2.8 says "a fixed small
  number" and does not choose. Needs a number before the scheduler is written.
- **Expansion horizon bounds.** §3.4 says −1 year / +1 year; whether that is
  right for a device that has been off for a season is untested.
- **Where the reparse sweep is triggered from.** The parser improving is a build
  event, not a data event, and nothing currently observes it.
- **Whether the recognizer's confidence bands survive contact with real
  captures.** The bands in §3.1.5 are reasoned, not measured. M1's captures are
  the sample to check them against.
