## 1. Finding them

- [x] 1.1 `projection::generations(base)` — the paths were built from a number and never enumerated, so nothing could ask what had accumulated. That is how every superseded generation came to be kept.
- [x] 1.2 Only names this module writes. A sidecar (`-wal`, `-shm`), a `state.notanumber.db` and anything else sharing the directory are left alone, and the test asserts they survive — a tidy that deletes from a directory it does not own is worse than the leak.

## 2. Removing them

- [x] 2.1 `projection::discard_superseded(base, current)` on session open, under the writer lock, beside the interrupted purge and the stale-projection rebuild.
- [x] 2.2 Keeps the current generation and one spare. A reader resolves the pointer and opens the file as two steps and §8.8 will not let it hold the writer lock across them, so deleting the moment something is superseded turns that gap into a failed read. With a spare, a read would have to be delayed across two full re-folds to be caught.
- [x] 2.3 A failure is logged and does not fail the open. A generation that cannot be removed is disk to reclaim next time, not a reason to refuse a store that is otherwise fine.

## 3. Seeing it

- [x] 3.1 Four rebuilds on a scratch store leave at most two generations. Before this, four.
- [x] 3.2 Asserted **by number, not by count** — "two files" would also be satisfied by keeping the wrong two. The current generation and its immediate predecessor are the survivors, and nothing older is.
- [x] 3.3 A store that has never been rebuilt keeps the one generation it has.
- [x] 3.4 Verified non-vacuous: stubbing the discard out fails both accumulation tests.
- [x] 3.5 The real archive confirms the shape read-only — its pointer is at generation 1 and `state.0.db` is still present from 2026-09-21. Not run against it: that takes the writer lock on somebody else's live store, and the next session they open does it anyway.

## 4. How this was found

- [x] 4.1 `tools/unwired.py` reported `discard_generation` as public and called only by tests, once it was taught that a function's own inline `#[cfg(test)] mod tests` is test code that happens to live in `src/`. Counting those as shipping callers is the difference between the check working and the check always passing.
- [x] 4.2 That one change took the gate from one finding to ten. Nine were real; one (`mutate::unflag`) was a false positive from the ambiguity rule, fixed by counting a qualified `module::name` call as naming that definition wherever it appears.
