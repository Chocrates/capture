## Why

The more precisely a date is written, the worse the archive reads it.

```
pay the invoice on friday      →  a task, due 2026-10-09
pay the invoice >2026-09-20    →  a day: 2026-09-20        — an EVENT
pay the invoice on 2026-09-20  →  a task, by its verb      — no date at all
pay the invoice on 20 sep      →  a task, by its verb      — no date at all
```

Three defects, found by trying to put one of each kind on a phone.

**1. The sigil produces the wrong kind.** `recognize` returns at the sigil path
with 100% confidence *before* `head_is_verb` is computed, so the composition
that makes a task never runs. §3.6 separates a thing you attend from a thing you
owe, and the most explicit instrument in the system — a sigil and a full date —
lands on the wrong side of that table. "Pay the invoice" becomes an appointment.

This is not the sigil rule being wrong. `recognizer/spec.md:6` says a sigil
always wins over inference, and it should: the sigil decides **which date**. The
verb decides **what kind of thing it is**. Those are different questions and
only one of them is being answered.

**2. A bare ISO date is invisible.** `dentist 2026-09-20` reads as "nothing
temporal". The only path that sees an ISO date at all is the `>` marker, which
then triggers defect 1.

**3. Only month-first parses.** `on sep 20` works. `on 20 sep`, `on 20
september` do not — they read as "by its verb" with no date. That is day-first
ordering, which is how most of the world writes dates.

## What Changes

- **A task verb composes with a sigil date**, as it already composes with an
  inferred one. The sigil still decides the date; the verb decides the kind.
- **A bare ISO date is recognised** in text, as a complete date.
- **Day-first month names parse**: `20 sep`, `20 september`, beside `sep 20`.
- **The confidence for a sigil date stays 100.** Nothing about how certain the
  date is changes; what changes is what the sentence is *for*.

## Capabilities

### Modified Capabilities

- `recognizer`: a sigil names a date and a verb names a kind, and both are read;
  an ISO date is a date wherever it appears; a month name parses either way
  round.

## Impact

- `crates/capture-parse/src/recognize.rs` — the sigil path composes rather than
  returning early; a complete-date anchor for ISO text; a day-before-month arm.
- `crates/capture-parse/src/token.rs` — if an ISO date needs its own class.
- Nothing downstream. No event kind, no body, no projection column, no schema.

## Non-goals

- **Numeric dates like `20/09` or `20.09.2026`.** `09/20` and `20/09` are the
  same characters meaning different days, and the archive has no way to know
  which was meant. A date it guesses wrong is worse than one it does not read:
  §3.1.5's rule, applied to the one case where being wrong is invisible.
- **Changing how a bare month name picks its year.** `on sep 20` said in October
  resolves to next September, deliberately — a forward marker landing in the
  past is a prompt that can never come due. It means a month name cannot express
  a date already gone, which is a real limit and a separate argument.
- **Times of day written exactly.** `at 14:00` already works; this is about
  dates.
