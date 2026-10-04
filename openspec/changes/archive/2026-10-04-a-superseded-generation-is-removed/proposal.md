## Why

Every projection rebuild leaves the old one on disk for ever.

`rebuild_projection` writes the new generation beside the current one and then
moves the pointer, which is right and the comment says why — never rename over
an open database, because readers keep the file they already opened and
SQLite's sidecars are named from the path. What it never does is come back for
the file it superseded.

`projection::discard_generation` exists, is documented *"Remove a superseded
generation"*, is tested, and is called by nothing. Found by `tools/unwired.py`
after it was taught to stop counting a function's own inline test module as a
caller.

On the owner's archive, which holds **zero notes**:

```
state.0.db  320 KB  Sep 21 21:22
state.1.db  320 KB  Sep 21 21:24
```

Generation 0 was superseded two minutes after it was written and has been there
for a fortnight. The projection is not small on a real archive — it holds every
note's text, the FTS index, every expanded occurrence — and a rebuild is not
rare: §1.3 makes "throw it away and compute it again" the entire migration
strategy, so every schema bump is a rebuild for everyone. `PROJECTION_SCHEMA`
is at 10 and rising, and this week added two of those.

So the growth is not a slow leak, it is one full copy of the archive's derived
state per schema version, kept for ever, on every device.

## What Changes

- **A superseded generation is removed**, by the function written to remove it.
- **Not immediately, and not all of them.** The current generation and the one
  before it are kept; anything older goes. A reader that resolved a generation
  a moment ago must still be able to open it, and keeping one spare is what
  makes the gap between "read the pointer" and "open the file" safe without a
  lock that §8.8 says reads must not take.
- **On opening a session**, under the writer lock that is already held, where
  every other deferred tidy in this system happens.
- **Failure to remove one is not a failure to open.** A generation that cannot
  be deleted is disk to reclaim later, not a reason to refuse the store.

## Capabilities

### Modified Capabilities

- `projection`: that a superseded generation does not accumulate.

## Impact

- `crates/capture-store/src/session.rs` — the sweep, beside the existing ones.
- `crates/capture-store/src/projection.rs` — a way to list the generations that
  exist, which nothing can currently ask.

## Non-goals

- **Deleting the generation a rebuild just replaced, immediately.** That is the
  race the "keep one spare" rule exists to avoid, and the comment on
  `rebuild_projection` already explains why writing beside and swapping is the
  shape.
- **Reclaiming space inside a generation.** `VACUUM` on the live projection is
  a different operation with different costs and is not what is leaking here.
- **Doing this from a read.** §8.8, and removing files is not a read.
