## Context

`capture-ffi` already asks the right question and says why
(`lib.rs:1280`): a round that never ran and a round that reached nobody need
different remedies, so they are asked separately. It then reads
`ops.last_round()` before looking at any peer.

`capture status` (`ops.rs:163`) and the desktop window (`ipc.rs:1340`) do not.
Both go straight to `ops.peer_addresses()` and report "device X has never been
reached", which is true and points at the network.

The owner's desktop is in exactly the state that distinguishes them: a paired
phone, and `capture-syncd` never started — correctly, since `install.sh`
declines to start a listening socket on anyone's behalf. The ledger records the
result as a question with a question mark, which is the whole finding.

Everything needed is already on `Ops` and already read-only: `last_round`,
`last_round_had_discovery`, `last_round_trouble`.

## Goals / Non-Goals

**Goals.** The three surfaces give the same answer. "Never run" is
distinguishable from "unreachable" everywhere. Read-only, so it works when the
writer does not (§8.8). One place decides what the words and thresholds are.

**Non-Goals.** Starting the daemon or offering to. A systemd unit. New
thresholds. Saying anything on an archive with no peers.

## Decisions

### One function, because this is about to be the fourth copy

The sigil mapping in the previous change was written six times and all six
agreed, which is the state that precedes them disagreeing. This is the same
shape: three surfaces answering one question, one of them correctly.

So the FFI's logic moves to `capture_store::sync_state`, returning a small
enum, and all three call it. The `duplicated` gate would catch a fourth
byte-identical copy and would not catch a fourth copy written in different
words — which is exactly how the current two came to be wrong.

```rust
pub enum SyncState {
    /// Nothing is paired. Not a failure and not reported.
    Alone,
    /// Paired, and no round has ever run.
    NeverRan,
    /// Rounds have run, but not for `since_ms`.
    NotRunning { since_ms: u64 },
    /// Rounds run and reach nobody for `since_ms`.
    ReachingNobody { since_ms: u64 },
    /// Running and reaching someone.
    Working,
}
```

The order of the arms is the order of the questions, and it is the point of the
type: `Alone` before anything, the job before the network, and the network
before the peer rows. A caller that matches exhaustively cannot skip the step
the current two skip.

*Rejected:* a boolean `sync_has_run()`. It is the same question asked worse —
every caller would then write its own thresholds and its own words, which is
how three surfaces came to disagree while all reading the same two fields.

*Rejected:* leaving the logic in `capture-ffi` and having the others call it.
`capture-ffi` is the Android binding; the command line depending on it to find
out whether sync ran would be the dependency arrow pointing at the phone.

### Thresholds and wording move unchanged

A day is noise because the period is fifteen minutes and Doze stretches it to
hours. Three days is not, because no plausible week has the desktop closed for
three days while the phone is in daily use. Those are argued in the FFI and are
being relocated, not revisited — a change that moved a threshold while moving
the code would make it impossible to tell which caused a later surprise.

### Read-only, and that is load-bearing

`Ops::open_read`. The moment this most needs to be answerable is when something
is wrong, and §8.8 puts reads off the writer for exactly that reason. The same
argument the staleness count was built on.

`capture peers` currently uses `Ops::open` (writable) where `open_read` would
do. Changed while here, because a read command holding a writable handle is the
§8.8 problem in miniature.

### The remedy is named, the action is not taken

`NeverRan` says to run `capture-syncd`. It does not start it, offer to start
it, or install a unit. `install.sh` declines on purpose — a listening socket is
switched on deliberately — and a health surface that quietly did what the
installer refused to would be worse than the silence it replaces.

## Risks / Trade-offs

- **A fourth surface could still skip the question.** → The enum's `Alone`
  arm and exhaustive matching make the step hard to omit, but nothing forces a
  new surface to call it at all. That is what the `unwired` gate is for.
- **`NotRunning` fires on a desktop that was simply shut.** → Same trade the
  FFI already accepted: a day is noise, three days is attention, and the text
  says when rather than asserting breakage.
- **Moving the FFI's inline logic risks changing phone behaviour.** → The arms
  are the existing branches one for one, and the phone's current wording is
  kept. A test pins that the phone still reports what it reported.

## Migration Plan

- **Log and projection:** untouched. This reads `ops`, which is local state.
- **Rollback:** reverting restores three surfaces, one of them right.

## Open Questions

- **Should `Working` be said at all, or only the failures?** Currently nothing
  is said when sync is fine, which matches "a list where everything is urgent
  is one where nothing is". Kept, but a person who has just started the daemon
  has no confirmation it took — and that is the `NeverConfirmed` case the
  *chord*'s health already treats as worth reporting.
- **Does `last_round_trouble` belong in this?** It is recorded and read
  somewhere already; whether the trouble string is worth surfacing beside
  these five states is a separate question about wording.
