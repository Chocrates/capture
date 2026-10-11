## 1. The order

- [x] 1.1 `Roster::admitted`: first-insertion HLC per key. Unit test that the
      second admission of a pair does not move the first device.

## 2. The rule

- [x] 2.1 `notifier::default_notifier` (pure) with tests for each scenario,
      sabotaged.
- [x] 2.2 `observes` reports `silent` when `alerts.silenced` is set.
- [x] 2.3 `speaks_here(paths, state, me, now)`; update session, FFI, overlay.
- [x] 2.4 `capture status`, FFI `notifier`: name the device the rule picks.

## 3. Proof

- [x] 3.1 Two-device test through `reconcile`: after pairing and a round, only
      the newer device speaks; mark it silent, sync, and the other speaks.
- [x] 3.2 `make ci`; Kotlin gate; ledger; archive.
