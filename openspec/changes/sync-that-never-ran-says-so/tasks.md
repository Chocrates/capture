## 1. One answer

- [x] 1.1 `SyncState` and `sync_state(ops, paired, now)` in `capture-store`, read-only, with the arms in the order the questions are asked: `Alone`, `NeverRan`, `NotRunning`, `NeverReachedAnyone`, `ReachingNobody`, `Working`.
- [x] 1.2 The thresholds and the wording moved from `capture-ffi` unchanged.
- [x] 1.3 Tested at each boundary — nothing paired, paired with no round, a round five days old, a round that reached nobody, two days quiet, four days quiet, and working.
- [x] 1.4 **Membership comes from the roster, not from `ops`, and the first draft had this wrong.** `peer_addresses` has a row only once somebody has been dialled or has answered — so a device paired and never yet contacted has a roster entry and *no row*, and inferring "is anything paired" from `ops` would have answered the exact case this exists for with silence. Caught by driving `capture peers`, which reads the roster, against a fixture that only had an `ops` row. Pinned by a test.

## 2. The three surfaces

- [x] 2.1 `capture-ffi` calls it instead of asking inline, and its `lasting` and `DAY_MS` are gone rather than left as a second copy.
- [x] 2.2 `capture status` asks it, **before** the peer rows. Both tiers are now declared together at the top, because the condition needs either and has to be said first.
- [x] 2.3 The desktop window asks it, with the same wording.
- [x] 2.4 `capture peers` says whether a round has run, beneath the rows.
- [x] 2.5 `capture peers` opens `Ops::open_read` rather than `Ops::open`.
- [x] 2.6 The FFI's `last_round_trouble` guard used a local `reached`; it now asks `SyncState` whether anything is working, which is the same judgement the condition above it already made.

## 3. The dangling command

- [x] 3.1 `capture admission-seen` said "give the key shown in capture health". There is no such command — `capture status` is the one that shows it.
- [x] 3.2 A test over this file's own message strings that every `capture <verb>` named in one is a verb the parser accepts. **It found a second case while being written** (`capture intent`, which exists but takes three arguments) and that is why it asserts the *verb* is known rather than that an invocation parses — a test that demanded a full parse would have been a second copy of every command's arity. Verified non-vacuous by putting `capture health` back and watching it fail.

## 4. Seeing it

- [x] 4.1 Reproduced on a scratch `CAPTURE_HOME`: a peer row and no round ever, and `capture status` now says **"sync has never run on this device, so nothing here exists anywhere else. Run capture-syncd to start it."** above the peer line rather than only "device d4d4d4d4d4d4 has never been reached."
- [x] 4.2 **Checked the real desktop, read-only, and the answer was not the expected one.** Its roster holds one key — its own. The phone was never re-paired after the 2026-09-21 fresh start, so the desktop is genuinely `Alone` and correctly says nothing about sync. The ledger line this change came from predates that reset. Worth recording plainly: the two devices are not paired, so nothing on either exists on the other, and that is a thing for the owner to do rather than a defect.

## 5. Found while checking

- [ ] 5.1 `capture status` reports "1 event(s) stored but not folded — this build has no rule for them" on the real archive, and `held_count` is the only accessor anywhere: no surface can list them, name the kind, or say why. A condition with nowhere to go.
- [ ] 5.2 The held event is the **roster key**, and the build knows exactly what that is. `ROSTER_KEY`, `DEVICE_RETIRE` and `DEVICE_READMIT` are missing from the fold's "does not affect note state" arm, so they fall to `Held::UnknownKind`. It appears after any rebuild — including the one `PROJECTION_SCHEMA` 10 is about to force on every archive. Its own change.
