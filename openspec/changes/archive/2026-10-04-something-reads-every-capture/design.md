## Context

`promote::sweep` is correct and has one non-test caller: `Session::open`, behind
a `catch_up` flag that `open_for_capture` turns off. So the sweep is a *side
effect of opening the store for writing*, and whether a capture gets read
depends entirely on what the next process to open the store happens to be.

The two rules that make this hard to fix are both right:

- **§8.8, "reads go direct, always."** A read must not need the writer, because
  the moment you most need to read is when the writer is broken.
- **`promote.rs`'s own module doc:** the sweep is *"never something a capture
  calls into"* — §3.1's gesture budget.

Between them they exclude "reads sweep" and "captures sweep", which is every
obvious answer, and the result has been that two commands grew the fix privately
(`cli::due` at `read.rs:1273`, `ffi::due` at `lib.rs:762`, each with a comment
describing this bug) and everything else stayed stale.

The constraint that resolves it: **sweeping and detecting-that-a-sweep-is-owed
are different operations with different privileges.** `Store::needing_parse` is
an indexed query on a read-only handle. So a read can be honest without being a
writer, and the writing can be given to a process that is already a writer.

## Goals / Non-Goals

**Goals.** A capture is read without the person asking. One named operation
instead of a copy-pasted session open. A read that says what it does not yet
know. §8.8 intact — no read takes the writer lock.

**Non-Goals.** Sweeping from the capture path. A scheduler, timer unit or
WorkManager job. Making the sweep faster or unbounded. Changing the recognizer,
the floor, or what promotion means.

## Decisions

### Three owners, because there are three kinds of process — not three mechanisms

The work is one function. What differs is who is already running:

| Where | Owner | Trigger | Before this |
|---|---|---|---|
| Desktop, overlay running | `capture-overlay` | its due tick | already worked |
| Android | the application | foreground; and the alarm receiver before asking what is due | already worked |
| Anywhere, daemon running | `capture-syncd` | every round, on the write nudge it already receives | **only if it had a peer** |
| Anywhere, neither | the person | `capture settle` | **nothing** |

So two of the four were already covered and the gap was narrower than the
symptom suggested — but it was the gap a person with one device and a terminal
falls into, which is every archive before a second device is paired.

All of them call the same `promote::settle(paths)`. The alternative — each
surface deciding for itself, which is the status quo — is what produced one
fixed command and six broken ones.

*Rejected:* a single universal mechanism. There is no process common to a
daemonless CLI and an Android app, and inventing one (a timer unit, a
WorkManager job) means a scheduler that can be silently not running — the exact
failure §8.8 is about, moved somewhere harder to see.

### A note records that it was read, even when reading found nothing

This was not in the plan and nothing works without it. A test asking "does the
staleness line clear?" said no, and the reason turned out to be that **the sweep
queue never empties.**

`needing_parse` reads "no promotion" as "not yet read". But the sweep only
writes a promotion when the floor is cleared, and `promote.rs` deliberately
declines to write one otherwise — an event per ordinary capture, to record that
the recognizer found what it usually finds. Both halves are reasonable and
together they mean an ordinary note is queued forever. With
`ORDER BY created_wall DESC LIMIT 200` on top, the sweep re-reads the newest 200
unpromotable notes on every open and never reaches anything behind them:

```
one dated task, then 250 ordinary notes, then five sweeps
  → tasks promoted = 0, still queued = 251
```

The task was first in and was never read. So the `LIMIT` was not a bound on work
per run, it was a horizon on the archive.

The fix is to record the negative result: a `read_by` table — `tgt`, the grammar
that read it, and the text position it was read at — written by the sweep for
every note it reads, promoted or not. A note is unread when it has no `read_by`
row, when that row names an older grammar, or when the text has moved since.

**A table, not a column on `notes`.** `projection.rs:118` states the rule and
says what it cost to learn: the schema is applied before its version is checked,
`CREATE TABLE IF NOT EXISTS` does nothing to an existing table, so a new column
never appears and the rebuild that would fix it cannot start. `parents` is a
table for this reason and so is this.

**Not in the log.** It is a fact about what a build has looked at, not about the
archive, and it does not replicate: another device running another grammar has
its own answer. `promote.rs` already rejected an event per ordinary capture.

**Discarded on a projection rebuild**, which is correct rather than merely
tolerable — a rebuild re-derives, and re-reading what the current grammar reads
costs one pass and changes no outcome. It is the same shape as a grammar bump,
which the sweep is already designed to spread over several runs.

**A note is read if *either* record says so** — a `read_by` row or a promotion
already current for this grammar. The first draft used `read_by` alone, and that
was nearly a serious bug: a rebuild drops `read_by` and re-folds `promotions`
from the log, so the first sweep after the 9 → 10 rebuild would have re-promoted
every note in the archive, written one duplicate promote event each, and synced
them all. The same on any second device, where a promotion arrives by sync and
no local read was recorded. Caught by a test comparing the log's event count
across a simulated rebuild, before this was committed.

*Rejected:* ordering the queue oldest-first. It changes which 200 notes starve.

*Rejected:* a sentinel promotion of `Promoted::Note`. That is the event per
ordinary capture, wearing the log's clothes.

### `settle` is a named operation, not an idiom

```rust
pub fn settle(paths: &StorePaths) -> Result<usize>
```

Opens a session, sweeps, returns how many were promoted. That is already what
`let _ = Session::open(paths.clone())?` does; the difference is that it has a
name, a return value, and one place to put the comment explaining why it exists.

The return value matters: `cli::due` currently writes `let swept = …;
drop(swept)` and therefore cannot tell "read, found nothing" from "could not
take the lock, answered from stale state". `Session::start` fails fast on
contention (`Writer::acquire` → `Ok(None)` → `Error::Usage`), so the caller can
always distinguish them and must.

### The daemon sweeps on the loop, not on the peer path

`round::tick` gives up before `reconcile` whenever there is no peer: no roster
secret (`round.rs:69`), no members (`:103`), or a `reachable` list the dial loop
finds empty (`:154`). So the sweep currently hangs off *having somebody to sync
with*, which has nothing to do with whether this device has unread captures.

It moves to `capture-syncd`'s loop, before `tick`. The daemon already holds a
`Watch` and already breaks out of its wait when this device has written
something — that is the 60s-budget trigger §2.5.1 asks for, and it exists.

*Rejected:* sweeping inside `reconcile`. That is where it accidentally is now.

### Reads report; reads never fix

A read asks `unread_count()` — read-only, one indexed query — and prints one
line when it is non-zero, naming `capture settle`.

It does not suppress the answer. "No tasks." and "No tasks. (3 captures not yet
read — `capture settle`)" are different claims, and the second is the true one.
Suppressing would make a read fail when the writer has not run, which is §8.8's
failure mode with the sign flipped.

*Rejected:* a read opportunistically trying the writer lock and falling back.
It passes §8.8's letter — it still works when the lock is held — but it makes
every read a potential writer, and a read that sometimes writes is one whose
cost and failure modes depend on timing. The honest line costs one query always.

### Android was already right, and the design said otherwise

Corrected after looking: `Archive::settle()` exists, its doc says *"called when
the application comes to the foreground"*, and `MainActivity.kt:119` calls it.
It opens a session, which sweeps. So the phone has owned this since it was
written and the hole was only ever on the desktop, where nothing owned it.

Two things were genuinely missing and they are small:

- `ffi::due` reimplemented the idiom inline instead of calling `settle()`. It
  stays — the alarm receiver arrives in the background with no guarantee the
  foreground pass has run, so a sweep there is load-bearing — but as one call.
- Nothing could ask, read-only, whether a surface is behind. `unread()` added.

The earlier draft of this document proposed adding a foreground settle that was
already there, which is worth leaving written down: the gap looked symmetrical
across platforms from the Rust side and was not.

## Risks / Trade-offs

- **A first sweep over an existing archive now actually does work**, where
  before it did nothing after the newest 200. On a large archive that is several
  rounds of real parsing where there used to be none — slower, and correct for
  the first time. The bound is what keeps any single round short.
- **`read_by` is projection state the fold does not produce.** → It is a cache
  of a pure function of (text, grammar), it is dropped on rebuild, and losing it
  costs one pass. Nothing reads it but the sweep queue and the count.
- **The daemon now holds the writer lock for a sweep it did not before.** →
  Bounded at `PER_SWEEP` = 200 and captures spool rather than fail while the
  lock is held, which is the behaviour the blob window already relies on. Worth
  measuring: 200 recognises plus writes, inside the lock, on the round. If it is
  long enough to be felt, the bound comes down — not the behaviour.
- **A staleness line on every read is a line that could become noise.** → It
  appears only when the count is non-zero, which on a working system is for the
  seconds between a capture and the next daemon wake. If it is persistently
  visible, that is the daemon not running, which is the thing worth seeing.
- **`--json` output gains a field.** → Additive, and a reader that ignores it
  behaves as before. A JSON consumer silently reading stale state is the same
  bug as a human doing it.
- **`capture settle` is a command a person should never need.** → True, and it
  is there so the staleness line has something to name, and so "nothing was
  read" can be told from "it read and found nothing". A diagnostic, like
  `capture due`.
- **Two writer-lock acquisitions per daemon round** (settle, then reconcile). →
  Cheap, and the alternative is threading a session through `tick`, which would
  make the sync round hold the lock across the sweep *and* the exchange.

## Migration Plan

- **Log:** nothing. No new kinds, no body changes.
- **Projection:** 9 → 10 with its changelog line, for the `read_by` table.
  Rebuilds on next open, after which every note is read once by the current
  grammar — which is what the old projection was failing to do.
- **Rollback:** reverting restores the status quo exactly — the ad-hoc sweeps in
  `cli::due` and `ffi::due` are the only behaviour that would be lost, and they
  are reinstated by the revert.

## Open Questions

- **Does the foreground settle belong in the Kotlin lifecycle or behind the
  FFI?** `make android-decides` says Kotlin decides nothing, which argues for
  the FFI exposing `settle()` and Kotlin only choosing *when*. "On foreground"
  is a platform fact, so that split looks right, but it is worth checking
  against the rule rather than assumed.
- **Should the staleness line appear on the phone too?** The spec requires the
  report where a surface would be affected. On Android the foreground settle
  means it should almost never be true, which is an argument both ways: rarely
  needed, and therefore meaningful when it fires.
- **Is `settle` the right name?** It is not `sweep` (an implementation), not
  `parse` (the recognizer's job), and not `sync` (taken). Open to a better one
  before it becomes a public FFI method.
