## Context

M1 is complete and in use on one machine: capture, store, fold, search, a GUI
covering the timeline, the note page and every mutation. Every capture is text,
and nothing reads it.

Two things exist that this milestone builds directly on. `capture-parse` holds the
recognizer's vocabulary, grammar and confidence bands (`DESIGN.md` §3.1.5) as
tested tables with no parser over them. `capture-core` holds a frozen encoding, a
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

### `capture-time` is a separate crate, `no_std`, and cannot read a clock

Recurrence expansion is the one place in this design where a wrong answer is
invisible: an occurrence at the wrong instant looks exactly like an occurrence.
Putting expansion in a crate that cannot reach a clock makes "the same rule
expands identically everywhere" a property the type system enforces rather than
a convention.

*Alternative considered:* expansion inside `capture-store`, where the clock already
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

### The due queue grows, and the overlay drains it

Review sessions and time-based intents join redaction deadlines in the existing
due-work queue. Every process startup remains the floor: that is the property
that matters, because a queue drained at startup can be late but cannot be
quietly dead, while a daemon that dies leaves deadlines unfired with nothing to
notice.

Startup alone is not enough for intents, though, and this is where they differ
from redaction. A 30-minute redaction window makes lateness invisible; a
reminder set for 3pm that fires at 3:40 because nothing was opened until then is
close to not firing at all.

So the resident overlay drains the queue on a timer. It is already running all
day for the chord, so this costs no new process and no new failure mode — and it
closes a gap that exists today: the overlay never opens a `Session`, so a
machine with the overlay running and nothing else touching the store leaves due
work undone until some command happens to run. The overlay is already the
elected writer under §8.8; this is it doing the job it already holds the lock
for.

*Alternative considered:* a real scheduler with wake-up timers for intents.
Rejected — it is a component that can be silently not running, which is the
exact property this design avoids, and it would need its own health surfacing to
compensate for that.

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

- **A pinned promotion is invisible and permanent.** `pinned: true` exempts a
  note from every future reparse, and nothing surfaces which notes are pinned.
  Anything that sets it wrongly — a bulk edit, an import — makes those notes
  permanently unreparseable with no way to notice. → Accepted for M2 rather than
  mitigated: the pin is only settable from an explicit correction path today.
  Revisit if a second author of promotions appears.

- **`carries_forward` defaults to false, and the class of thing that needs it is
  exactly the class where forgetting hurts most** — a missed dose, an unpaid
  invoice. → The flag is never inferred, and a repeatedly-lapsing series prompts
  once in review rather than the system guessing.

## Migration Plan

No data migration. Every new capability is additive: existing notes are
unpromoted, existing projections rebuild into the widened schema, and a build
without these kinds holds them rather than misreading them.

Order of work, each step usable before the next:

1. `capture-time` — instants, floating dates, RRULE subset, DST policy. No
   dependants yet.
2. New event kinds and bodies in `capture-core`, with golden vectors. Fold rules
   and conflict rules with property tests. Nothing authors them yet.
3. The parser in `capture-parse` over the existing tables, returning candidates.
4. Promotion: authoring `promote`, the reparse path, pinning.
5. Projection: occurrence expansion, the widened due queue.
6. Surfaces: agenda and calendar in the desktop, then the CLI equivalents.
7. Review, intents, activation — each depends on the due queue from step 5.

Rollback is per-step: nothing before step 4 writes an event, and steps 1–3 are
inert additions.

### A review session is five items

§2.8 asks for "a fixed small number" and declines to pick one. Five.

The property that matters is that the session is always finishable, because a
session you do not finish is a backlog by another name and §3.6 spends its
length on why manufactured backlogs are the wrong shape for this user. Five is
small enough to always complete and still reaches two or three of §2.8's four
selection tiers on an ordinary day. Ten reaches further down the priority order
but is abandonable halfway; three guarantees completion and means most of the
selection logic never runs.

Deliberately not a setting. A knob nobody turns is a default with extra code,
and §0.2 rates the cost of decisions highly enough that offering one here is not
free.

### The expansion horizon stays ±1 year, and gets measured

§3.4's bounds are kept rather than re-guessed. This is a performance choice
rather than a correctness one — failure mode 23 already establishes that an
exhausted horizon costs a brief expansion on first open and never missing or
wrong data, because the rule is the truth and the expansion is not.

An unbounded daily series is roughly 730 rows inside that window, recomputed on
every rebuild. Whether that stays comfortable with a dozen such series is
measured during M2 rather than argued about now.

### A reparse sweep is triggered by a grammar version, on startup

Failure mode 20b establishes that reparsing the backlog is *safe* — pins are
never overridden — and says nothing about when it happens. Nothing in the system
observes a build, so without a trigger a note captured in March could never
benefit from a grammar fix shipped in June.

So `capture-parse` carries a version, bumped whenever the vocabulary or grammar
changes, and every promotion records the version it was parsed under. Startup
reparses anything parsed under an older one. The trigger becomes a fact in the
data rather than an event nobody observes, and the cost is the one §2.5.1
already anticipated: one `promote` per note per sweep.

### The confidence bands are provisional, and only loosen against evidence

The bands in §3.1.5 were reasoned, not measured — there was no parser and no
corpus when they were written. They decide what silently becomes a calendar
entry and what waits as a suggestion, so being wrong in the generous direction
produces exactly the false positives §3.1.3 calls the worse error.

They ship as written and are treated as strict. A band moves only when a real
capture forces it, and the capture that forced it is pinned alongside as a test
case. That way the bands accumulate evidence in the direction the design already
prefers — a miss costs a glance at a suggestion, a false positive costs a
calendar entry.

## Open Questions

None outstanding. The four that were open — session size, horizon bounds, the
reparse trigger, and what to do about the unmeasured confidence bands — are
settled above.
