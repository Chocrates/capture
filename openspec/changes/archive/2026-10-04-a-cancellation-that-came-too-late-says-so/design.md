## Context

The fold resolves a note's governing redaction, then branches on whether the
content is already gone. The destroyed branch sets `redacted_at` and ignores
`r.cancelled`, so the combination — destroyed *and* withdrawn — is computed and
thrown away.

## Goals / Non-Goals

**Goals.** Keep it. Report it. Offer nothing that cannot be done.

**Non-Goals.** Recovering the content. Preventing the race. Other surfaces.

## Decisions

### On the note, not in a log of events

The condition is a fact about a note's current state, which is what the fold's
`Note` is for, and a surface asking "is anything wrong with this archive" wants
it keyed by the thing it is wrong about. An event-shaped record would need its
own ordering rules to answer the same question.

### A new table, because the schema may add tables and never columns

`projection.rs:118` states the rule and what it cost to learn. So the fact goes
in a table of its own rather than a column on `notes`, and `PROJECTION_SCHEMA`
moves so every projection is rebuilt with it.

### Reported without a remedy

There is nothing to undo. The report says what happened and names the note, and
deliberately does not point at `capture recover` — a snapshot may or may not
exist, that is a separate mechanism with its own window, and suggesting it here
would imply the two are connected.

### Not an error

Nothing failed. Two devices disagreed about timing, which is what a distributed
archive does; §1.1 asks for it to be visible, not prevented.

## Risks / Trade-offs

- **The condition never clears.** → It is a historical fact and there is nothing
  to resolve. If it becomes noise on an archive where it happens often, that is
  a signal about the window rather than about the report.
- **A rebuild to add a table.** → The twelfth, and §1.3 makes it the strategy.

## Migration Plan

`PROJECTION_SCHEMA` 11 → 12. The condition is derived from the log, so a rebuild
finds every past instance rather than only new ones.

## Open Questions

- **Should it clear once acknowledged?** `capture status` has an acknowledgement
  shape for other conditions. Worth doing if it turns out to recur.
