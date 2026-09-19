## 1. The ones that already disagree

- [x] 1.1 `unhex`: keep the copy that refuses an empty string, delete the other, and write the reason in the survivor — both callers sit either side of §5.3's two-signature readmission, and a request decoding to nothing should be refused rather than carried as a zero-length key. **Already done for the two inside `capture-ffi`**; the copy in the command line's `pair.rs` remains.
- [x] 1.2 `hexed`: one definition, in `capture-core` beside the key type.
- [x] 1.3 A test that the two agree on the cases that differ — empty, odd length, non-hex — so a future merge cannot quietly pick the other behaviour.

## 2. The zone

- [x] 2.1 One `local_zone`, in `capture-store`'s `notifier`, beside the rule that needs it. Four copies today: `notifier.rs`, cli `read.rs`, cli `write.rs`, overlay `intents.rs`.
- [x] 2.2 The three others call it. The overlay's copy was made this week by reading the command line's twelve lines away — worth saying in the commit, because the mechanism is the point.
- [x] 2.3 A test over the parse: `$TZ` set, `$TZ` empty, a symlink with `zoneinfo/` in it, a symlink without, neither. A wrong answer moves quiet hours by the UTC offset.

## 3. The short key

- [x] 3.1 One rendering, in `capture-core`. Seven copies today, and it is what a person compares when deciding whether to retire a device — two surfaces disagreeing about how many bytes to show is a disagreement at exactly the wrong moment.
- [x] 3.2 Every caller uses it: `round.rs`, cli `ops.rs`, cli `pair.rs`, desktop `ipc.rs`, and three in `capture-ffi`.
- [x] 3.3 Assert the length in one test rather than in seven places by implication.

## 4. Opening the current generation

- [x] 4.1 One call for "the current generation, read-only", replacing the ~20 hand-written copies. It encodes a rule — the current generation is whatever `ops` says — and a site that opened a stale path would read an archive that is real, consistent, and out of date.
- [x] 4.2 Converted the reads of the *current* generation: the FFI's `read()` and its staleness check, the overlay's intent tick and text scale, the command line's reader, the desktop's `read_only`, and `promote::reading`. Left alone, deliberately: the writable opens in `Session::open` and `capture init`, and the opens of a *specific* generation right after `set_generation(next)` in rebuild and restore. Those name a generation on purpose, which is the distinction the helper exists to make visible.

## 5. The test fixture

- [x] 5.1 Not needed in the end. The two `store()` helpers that were genuinely identical turned out to be the same hand-written generation open as everywhere else and now call `projection::current`; the `Scratch` fixtures differ per suite in ways that are the point of each suite. The four `now()` helpers are allow-listed with the reason: a test comparing against real instants wants the clock *it* reads, and there is no decision in `SystemTime::now` to centralise.
- [x] 5.2 Leave `capture-ffi`'s alone, or make a second honestly-named one — it opens an `Archive` rather than a store, which is a different thing wearing the same name (open question 2).

## 6. Stopping it happening again

- [x] 6.1 A new `tools/duplicated.py` rather than extending `tools/unwired.py` — which already refuses a public item nothing calls — to refuse a second definition of a name defined elsewhere in the workspace, with an allow-list for the names a type is expected to have (`new`, `open`, `fmt`, `default`).
- [x] 6.2 Run it against the tree as it stands and read every hit. The allow-list is written from what it finds, not guessed in advance.
- [x] 6.3 Decide `ci` or `accept` (open question 1). A check that fires on a legitimate second `fn new` mid-refactor is one somebody learns to skip.

## 7. Closing

- [ ] 7.1 `make ci`.
- [ ] 7.2 Confirm the count is zero by re-running the searches in the proposal, and put the numbers in the commit — before and after, so the next audit has a baseline.
