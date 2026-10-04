## 1. The purge

- [x] 1.1 Delete the queue row in `purge_note`, beside the other derived rows it already removes.
- [x] 1.2 **The first attempt put it in the wrong function.** There are two `DELETE FROM ambient` lines — one in the write-path cleanup and one in `purge_note` — and anchoring on the first match landed the new delete beside a `redact_queue` delete that was already there, duplicating it in the path that did not need it. The tests still failed, which is how it was caught; the fix is anchored on the *last* occurrence instead.

## 2. Seeing it

- [x] 2.1 After a drain, nothing is queued.
- [x] 2.2 A second drain destroys nothing. It used to destroy it again — intent recorded, snapshot attempted, rows already gone deleted, an UPDATE that changed nothing, and a write-ahead-log checkpoint, on every drain for every redaction the archive had ever carried out.
- [x] 2.3 Other queued destructions are untouched, which is the guard against clearing the whole queue rather than one row.
- [x] 2.4 Cancelling after the destruction is refused and says it may already have been carried out. It used to find the stale row, write the cancellation, and report that nothing was destroyed over content that was gone.
- [x] 2.5 Cancelling inside the window still works, so the refusal above is not cancelling being broken.
- [x] 2.6 Non-vacuous, with the sabotage confirmed to have left `purge_note` before the result was trusted.

## 3. Found while finishing

- [x] 3.1 `make duplicated` caught `body_survives` written identically in two of my own test suites. Given one home in `tests/common/mod.rs` — a subdirectory because Cargo compiles every top-level file in `tests/` as its own binary. The module is deliberately small: each suite's fixtures differ in ways that are the point of that suite, and only a question with one answer belongs there.
