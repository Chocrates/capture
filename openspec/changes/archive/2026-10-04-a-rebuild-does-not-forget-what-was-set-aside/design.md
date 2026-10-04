## Context

Three writers put rows in `quarantine`, all in `gate.rs`, all at ingest.
`projection::rebuild` folds `Log::all`, which excludes set-aside events by
definition, and writes no quarantine rows at all.

`Log::is_aside` reads the authoritative column and is called by nothing, which
is what surfaced this.

## Goals / Non-Goals

**Goals.** A rebuilt projection reports what the log has set aside. The
re-admission remedy survives a rebuild. One place decides, so a new caller
cannot forget.

**Non-Goals.** Changing when the gate sets `aside`. A `first_seen` column on the
log. Restoring only `post_retire`.

## Decisions

### Inside `rebuild`, which means a parameter

Repopulating from `rebuild_projection` instead would leave `crash-victim` and
every other producer of a projection making an incomplete one. A projection is
either complete or it is a trap, and "the caller remembers" is the arrangement
that produced this bug.

So `rebuild` takes the aside rows. There are eleven call sites; the four
production ones read them from the log, and the synthetic tests pass none,
which is honest — a test that builds a list of envelopes by hand has no log and
nothing set aside.

*Rejected:* `rebuild` taking a `&Log`. Several tests construct events without
one, and taking a log to read one column would make them unable to call it.

### `first_seen` becomes the authored instant

The quarantine row wants a moment. The log knows when the event was *authored*;
when this device first *saw* it existed only in the projection being replaced.

Using the authored instant means a rebuilt row may sort differently from the one
it replaces. That is the better failure: it is stable, so two rebuilds produce
the same order, where "now" would bunch every restored row at the rebuild and
destroy the ordering entirely.

*Rejected:* an `aside_at` column on `events.db`. A schema change to the log — the
one database that is never thrown away — to order one diagnostic.

### All reasons, not just `post_retire`

`reconsider` only reads `post_retire`, but `capture quarantine` reports every
reason and all of them are equally missing. Restoring one and not the others
would make the surface right about retirement and wrong about collisions.

## Risks / Trade-offs

- **A restored row's moment is not when it was seen.** → Named above, written in
  the spec, and the alternative is a column on the log.
- **An extra query per rebuild.** → Against a full re-fold of the log.
- **Eleven call sites change.** → Four matter; the rest pass an empty slice,
  which is what they mean.

## Migration Plan

No schema change: the `quarantine` table already exists and already has these
columns. The first rebuild after this change restores whatever the log has
marked — including rows lost by a rebuild performed before it, because the
authoritative mark was never the thing that went missing.

## Open Questions

- **Should `capture status` distinguish "set aside" from "held"?** They are
  different tables answering different questions — refusing to use an authentic
  event, against having no rule for one — and `capture held` now exists for the
  second. Whether the first deserves the same treatment is a surface question.
