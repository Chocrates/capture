## Context

`rebuild_projection` writes generation `n+1`, points `ops` at it, and returns.
The file for `n` stays. `discard_generation` was written to remove one and has
never been called from shipping code.

The evidence is on the owner's disk: `state.0.db` and `state.1.db`, both 320 KB,
on an archive with no notes in it, two minutes apart in September.

## Goals / Non-Goals

**Goals.** Bounded disk. No new race. No lock on a read.

**Non-Goals.** Immediate deletion after a swap. `VACUUM`. Anything on a read
path.

## Decisions

### Keep the current generation and one spare

A reader does two things that are not atomic together: it asks `ops` which
generation is current, and it opens that file. §8.8 keeps reads off the writer
lock, so there is no way to make those one step — and that is the right trade,
because a read that needs the writer is a read that fails when the writer does.

So between those two steps the generation may be superseded. Deleting the file
the moment it is superseded turns that window into a failed read.

Keeping one spare closes it for any realistic reader: to be caught, a read would
have to resolve generation `n`, then be delayed across *two* rebuilds before
opening it. A rebuild is a full re-fold of the log, so that is not a window, it
is a hibernation.

*Rejected:* a lock around resolve-and-open. It is §8.8's rule inverted for the
benefit of a disk-space tidy.

*Rejected:* deleting on a timer, or on a "generation is old enough" clock. A
clock here would be a second rule about the same thing, and the count already
answers it exactly.

### Under the writer lock, on session open

Every other deferred tidy in this system is there — the interrupted purge, the
stale-projection rebuild, the sweep, the due drain. It costs a directory listing
on a path that is already doing several, and it means the tidy runs on exactly
the processes that are entitled to change the store.

### A failed removal is not a failed open

A file that is locked, read-only or gone is not a reason to refuse a store that
is otherwise fine. §8.8's argument about diagnostics applies to the whole
startup path: the operation somebody is actually trying to do must not be held
hostage to housekeeping.

## Risks / Trade-offs

- **A reader delayed across two rebuilds gets a missing file.** → Named above
  and accepted; the alternative is a lock on a read. The failure is a clear
  error on one read rather than silent wrong data.
- **Listing a directory on every session open.** → It is a handful of entries
  beside a full-text index being opened.
- **An external tool pointed at an old generation loses it.** → It was already
  superseded derived data, reproducible by `capture rebuild`, and §1.3 makes the
  projection disposable by design.

## Migration Plan

Nothing to migrate. The first session open after this removes whatever has
already accumulated — on the owner's archive, `state.0.db`.

## Open Questions

- **Should `capture status` report reclaimable space?** It reports other
  deferred work. Probably not worth a line that is almost always zero once this
  exists.
