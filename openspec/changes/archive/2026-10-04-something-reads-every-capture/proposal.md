## Why

A capture is not promoted until something opens a session afterwards, and on a
single-device archive nothing does. The effect, reproduced on a scratch store:

```
$ capture + 'call the dentist tomorrow'
$ capture tasks   → No tasks.
$ capture tasks   → No tasks.
$ capture pin <slug>          # any write at all
$ capture tasks   → call-dentist-tomorrow-…   task · due 2026-10-04
```

So a person who captures and then looks sees nothing, twice, and the thing that
fixes it is unrelated to what they were doing. §3.1 is why: parsing happens
*after* the commit so the gesture never waits for the recognizer. That is right.
What is wrong is that nothing was given the job of doing the parsing.

`promote::sweep` has exactly one non-test caller — `Session::open`, behind a
`catch_up` flag — so the sweep is a side effect of opening the store for
writing. Reads do not open a session, by design: §8.8 says *"Reads go direct,
always… diagnostics that stop working when the system is unhealthy are
diagnostics you cannot use at the moment you need them."* And `promote.rs` says
the sweep is *"never something a capture calls into."* Both of the obvious fixes
are therefore closed off, which is why this has survived.

Three distinct gaps, each confirmed in the code:

1. **The daemon never sweeps on a single-device archive.** `capture-syncd` only
   reaches a session through `round::reconcile`, and `round::tick` gives up
   before reaching it whenever there is no peer to exchange with — no roster
   secret (`round.rs:69`), no members (`:103`), or a `reachable` list that
   leaves the dial loop with nothing to iterate (`:154`). One device with the
   daemon running is swept never. §2.5.1 lists `parse` as an
   enrichment class triggered immediately with a 60s budget, and nothing runs
   it.

2. **Two read commands already work around this, ad hoc and inconsistently.**
   `cli::due` (`read.rs:1273`) and `ffi::due` (`lib.rs:762`) each open a throwaway
   writer session with a comment explaining this exact bug. Whichever command
   bit its author got the fix; `tasks`, `agenda`, `log`, `search` and the entity
   pages did not. Worse, `cli::due` discards the result — `let swept = …;
   drop(swept)` — so when the writer lock is held it silently reports stale data.
   That is §8.8's named failure, in the one command whose whole purpose is
   answering "why did nothing happen".

3. **A read cannot say that it is stale,** though it has everything needed to.
   `Store::needing_parse` is an indexed projection query on a read-only handle.

4. **The sweep is inert on any archive over 200 notes, and this is the severe
   one.** Found by a test written for gap 3, not by looking for it.

   Nothing records that a note was *read and found to be nothing*. A promotion
   is only written when the floor is cleared, and `promote.rs` declines to write
   one otherwise — rightly, since that would be an event per ordinary capture.
   But `needing_parse` treats "no promotion" as "not yet read", so **every
   ordinary note stays in the queue permanently**, and the queue is
   `ORDER BY created_wall DESC LIMIT 200`.

   So the sweep re-reads the same newest 200 unpromotable notes on every session
   open, forever, and never reaches anything behind them. Measured on a scratch
   store — one dated task, then 250 ordinary notes, then five sweeps:

   ```
   tasks promoted = 0, still queued = 251
   ```

   The task was captured *first* and was never read at all. On a real archive —
   where ordinary notes are most captures — the sweep does no useful work after
   the first 200 notes, and a grammar improvement reaches nothing. The bound was
   meant to spread work across runs (`promote.rs:231`); without a record of a
   negative result it is a horizon instead.

## What Changes

- **A note records the grammar that read it, whatever reading found.** This is
  what makes the queue finite. A new `read_by` table in the projection, written
  by the sweep — a table and not a column on `notes`, because the schema may add
  tables and never columns (`projection.rs:118`, and `parents` is there for the
  same reason). Discarded on rebuild, which costs one pass over the archive and
  no correctness.
- **The daemon sweeps every round, whether or not it has peers.** Moved off the
  peer path onto the loop itself, before the early returns. It already holds a
  `Watch` that wakes it when this device writes, so a capture settles on the
  next wake rather than on the idle timer.
- **One named operation replaces the copy-pasted session open.** `settle()` on
  the store, on the FFI, and as `capture settle`. The two existing workarounds
  call it instead of reimplementing it.
- **Android needs almost nothing**, which was not what this proposal first
  claimed. `Archive::settle()` already exists and `MainActivity.kt:119` already
  calls it on foreground. `ffi::due` is changed to call it rather than
  reimplement it inline, and `unread()` is added so a surface can report being
  behind. The hole was only ever on the desktop.
- **Reads report staleness; they never fix it.** A read command that finds
  unread captures says how many, in one line, and names `capture settle`.
  Read-only, costs one indexed query, and works when the writer is down —
  which is §8.8's argument rather than a concession to it.
- **`cli::due` stops swallowing the failure.** If it cannot settle it says so
  instead of answering from stale state.

## Capabilities

### New Capabilities

None. This gives an existing job an owner.

### Modified Capabilities

- `promotion`: that a capture SHALL be read without any further action by the
  person who wrote it, and which process owes that.
- `cli`: `settle`, and that a read SHALL report unread captures rather than
  silently answering from stale state.
- `projection`: that a capture records having been read, so the queue of
  unread captures is finite; and that it is countable from a read-only handle.

## Impact

- `apps/capture-syncd/src/main.rs` — sweep on the loop, not the peer path.
- `crates/capture-store/src/promote.rs` — `settle(paths)`, the one way to do this.
- `crates/capture-store/src/projection.rs` — the `read_by` table, the shared
  unread predicate, `unread_count`. `PROJECTION_SCHEMA` 9 → 10.
- `apps/capture-cli/src/read.rs`, `write.rs`, `args.rs` — `settle`, and the
  staleness line on reads.
- `crates/capture-ffi`, `android/` — `settle()`, called on foreground.

## Non-goals

- **Sweeping from the capture path.** §3.1's budget, and `promote.rs` says it
  outright. The capture writes and returns; something else reads.
- **Making reads take the writer lock.** §8.8. A read that needs the writer is a
  read that fails when the writer does.
- **A timer unit, launchd job or WorkManager task.** The daemon and the
  foreground are already running at the moments that matter. Scheduling is a
  separate argument about what may wake a device, and this change needs none of
  it.
- **Backfilling an archive faster than `PER_SWEEP`.** 200 per sweep is
  deliberate (`promote.rs:235`); a grammar bump spreading over several rounds
  costs nothing and keeps any one round short. Once the queue is finite the
  bound does what it was written to do.
- **Recording the read in the log.** An event per ordinary capture, which
  `promote.rs` already rejected and which would replicate a fact that is local
  to a build.
- **Reading the backlog oldest-first instead.** It changes which 200 notes are
  starved, not whether any are.
