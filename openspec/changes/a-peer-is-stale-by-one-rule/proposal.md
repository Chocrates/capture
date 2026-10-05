# a-peer-is-stale-by-one-rule

## Why

"A decision made twice is a decision that can be made differently." Whether a
peer has gone quiet is decided twice, identically, in two surfaces:

```rust
// apps/capture-cli/src/ops.rs
const PEER_STALE_MS: u64 = 7 * 24 * 60 * 60 * 1000;
let stale = p.last_seen.is_none_or(|t| now.saturating_sub(t) > PEER_STALE_MS);

// apps/capture-desktop/src/ipc.rs
const PEER_STALE_MS: u64 = 7 * 24 * 60 * 60 * 1000;
let quiet = k.last_seen.is_none_or(|t| now.saturating_sub(t) > PEER_STALE_MS);
```

Two constants and two predicates for one question. Change the week in one and
the terminal and the window disagree about whether your phone needs attention —
and neither is obviously wrong, which is the state that makes somebody stop
trusting the report rather than the surface.

`duplicated.py` cannot see it: it compares function bodies, and these are a
constant and an inline expression. That is worth saying because the gate's own
docstring claims this category, and this is a case it misses by construction.

**And nothing tests the boundary.** No test asserts that six days is not stale
and eight days is, that a peer never reached counts as stale, or that the tier
is "needs attention". `syncstate.rs`'s tests cover the *round* state — alone,
never ran, reaching nobody — which is a different question about this device
rather than about a peer.

## What Changes

- One `peer_is_stale(last_seen, now)` in `capture-store`, beside `sync_state`
  and `lasting`, which are already the shared vocabulary for reporting sync.
- Both surfaces call it. Neither keeps a constant of its own.
- Tests for the boundary: inside a week, outside a week, exactly a week, and
  never reached.

## Capabilities

### Modified Capabilities

- `sync-discovery` — a peer is stale by one rule, in one place.

## Non-goals

- **No change to the week.** Seven days stays seven days; this is about where
  that decision lives, not what it is.
- **No change to the wording.** The two surfaces phrase the report differently
  on purpose — one writes a line in a terminal, the other fills a condition with
  a tier — and only the predicate is shared.
- **Not `notifier::STALE_MS`.** It is also a week and it is a different
  decision: how late a *moment* may be and still be worth announcing. It has its
  own reasoning written out, and merging the two because the numbers match would
  be the mistake this change is trying to prevent, pointed the other way.
