## Why

This is in the owner's own ledger, written as a question because the system
would not answer it:

```
## Open
- Phone last reached by desktop 51h ago (as of 2026-09-21 21:xx) — sync not running?
```

It was not running. `capture-syncd` has never been started on that desktop —
deliberately, because `install.sh` says sync is opt-in and "opens a listening
socket, and that is a thing to switch on deliberately". So the daemon being off
is correct. What is wrong is that **no surface on the desktop says so**, and the
one that is closest says something that points the wrong way.

`capture status` and the desktop window both report only:

```
device d4389d6f has never been reached.
```

Which reads as a network problem — a VPN, a guest network, discovery blocked —
when the cause is that nothing has ever tried. The remedy for one is to change
the network; for the other it is to run a daemon. The surface cannot tell them
apart, so the person cannot either, and the ledger entry above is what that
costs: a guess, with a question mark, left open.

**The phone already gets this right**, and `capture-ffi` explains why at
length (`lib.rs:1280`):

> A round that ran and reached nobody and a round that never ran are different
> situations with different fixes, and the peer rows report them identically.
> So they are asked separately…

It then asks `ops.last_round()` before looking at any peer, and says *"sync has
never run on this device, so nothing here exists anywhere else"*. The reasoning
is already written down and already implemented. It is implemented **once**, on
the surface the owner was not looking at.

The data is there on every device: `last_round`, `last_round_had_discovery` and
`last_round_trouble` are all on `Ops` and all readable read-only. Two of three
surfaces simply never ask.

## What Changes

- **One answer to "what is the state of sync here", in `capture-store`.** The
  question the FFI already asks, moved somewhere all three surfaces can ask it,
  so there is one place that decides what "never run" means rather than three
  that could disagree. The FFI's existing wording and thresholds are the
  behaviour being moved, not redesigned.
- **`capture status` and the desktop window ask the round question first.** The
  job before the network: if rounds are not happening, what the network would
  have done is not a question anybody can act on.
- **A paired device with no round ever is reported as attention**, and names
  the remedy — the daemon — rather than describing the peer.
- **`capture peers` says whether a round has run**, above the per-peer rows,
  for the same reason: "never reached" beside "sync has never run" is a
  complete sentence, and alone it is misleading.
- **`capture health` stops being named by an error message**, because there is
  no such command. `capture status` is the one that shows what the message is
  pointing at.

## Capabilities

### Modified Capabilities

- `cli`: that a surface reporting unreached peers SHALL first report whether
  sync has run at all, and that the CLI names only commands it has.
- `browse`: the same requirement where the window reports peers.

## Impact

- `crates/capture-store/src/round.rs` or a small module beside it — one
  function answering the state of sync from a read-only `Ops`.
- `crates/capture-ffi/src/lib.rs` — calls it instead of asking inline.
- `apps/capture-desktop/src/ipc.rs`, `apps/capture-cli/src/ops.rs` — ask it.
- `apps/capture-cli/src/args.rs` — the dangling `capture health`.

## Non-goals

- **Starting the daemon, or offering to.** `install.sh` refuses on purpose and
  that reasoning stands: a listening socket is switched on deliberately. This
  change makes the state visible; acting on it stays the owner's.
- **A systemd unit.** Same argument, and it is a packaging decision rather than
  a correctness one.
- **Changing the thresholds.** A day is noise, three days is not; those numbers
  are argued in `capture-ffi` and are being moved unchanged.
- **Reporting sync state where nothing is paired.** One device with no peers is
  not failing to sync, and saying so on every single-device archive is how a
  reader learns to skip the section.
