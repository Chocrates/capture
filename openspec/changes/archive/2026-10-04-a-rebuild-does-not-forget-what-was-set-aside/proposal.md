## Why

A projection rebuild loses every record of what this device has set aside, and
one of the things that breaks is a remedy the design went out of its way to
make local.

The log's `aside` column is durable: an event marked there occupies its position
and is excluded from every fold. The projection's `quarantine` table is the
*report* of that, and it is written only by `Gate` at ingest — three call sites,
all in `gate.rs`. `projection::rebuild` never writes it, and the events it folds
come from `Log::all`, which excludes anything set aside by definition.

So after `capture rebuild`, or after a projection is deleted — which §8.8 and
the failure table both declare safe — the table is empty while the log is
unchanged. Two things follow:

1. **`capture quarantine` and `capture status` report nothing set aside**, while
   events remain set aside, invisible to every surface and excluded from every
   fold. The one surface whose job is to say "this device is holding something
   it will not use" says nothing.

2. **The re-admission remedy stops working.** `Gate::reconsider` iterates
   `quarantined_at("post_retire")` to decide what a reissued retirement should
   release. With no rows it finds nothing, so reissuing a retirement with a
   later head no longer re-admits the events below it.

That second one is the sharper half, and `gate.rs` says why in its own comment:

> Before these were stored at all, this depended on a peer offering them again:
> the remedy for "keep those after all" required the retired device's neighbour
> to still be reachable and still be holding them.

Storing them locally is what removed that dependency. A rebuild silently puts it
back — and a rebuild is not rare, because §1.3 makes "throw it away and compute
it again" the whole migration strategy, so every schema bump is one for
everybody.

Found by `tools/unwired.py` reporting `Log::is_aside` as public and called only
by tests. It is the only read of the authoritative column, and nothing consults
it.

## What Changes

- **A rebuild repopulates `quarantine` from the log**, which is where the
  authoritative record already is. The table goes back to being what it always
  claimed to be: a report of the log rather than a second place the truth lives.
- **Inside `rebuild`, not beside it.** Every call site that produces a
  projection has to produce a complete one, or the next caller added is the one
  that forgets — which is how this happened.
- **`first_seen` for a rebuilt row is the event's own timestamp**, not the
  moment this device saw it. That moment only ever existed in the projection,
  which §1.3 makes disposable; the authored instant is in the log, is stable
  across rebuilds, and orders the report the same way every time.

## Capabilities

### Modified Capabilities

- `projection`: that a rebuilt projection reports what the log has set aside.

## Impact

- `crates/capture-store/src/log.rs` — the aside rows, read through the column
  `Log::is_aside` was written for.
- `crates/capture-store/src/projection.rs` — `rebuild` takes them and writes
  them.
- Eleven call sites, four of them production.

## Non-goals

- **Changing what `aside` means or when the gate sets it.** The marking is
  correct; only the report is lost.
- **Recording when a device first saw an event in the log.** It would be a
  column on `events.db` for a field used to order one diagnostic, and the
  authored instant answers it well enough. Named in the design rather than done
  quietly.
- **`post_retire` only.** All reasons are restored, because all of them are
  reported by `capture quarantine` and all of them are equally absent now.
