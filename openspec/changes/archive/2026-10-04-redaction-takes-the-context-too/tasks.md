## 1. The purge

- [x] 1.1 Delete the ambient row in `purge_note`, beside the mentions and list items it already deletes. The third table found surviving a purge, for the same reason as the two before it: a purge edits the projection in place, so every derived table has to be named or it is simply missed.

## 2. Seeing it

- [x] 2.1 A purged note has no ambient row. The test first asserts the capture *recorded* a context, so it cannot pass by there being nothing to delete.
- [x] 2.2 A destroyed note is no longer returned by the context query that matched it before. The repository is read back from what the capture stored rather than hardcoded — whatever context was recorded is the context that must go.
- [x] 2.3 Another note's context is untouched, which is the guard against over-deleting.
- [x] 2.4 **Verified non-vacuous on the second attempt.** The first check silently removed nothing — the escaped quotes in the edit did not match the source — and the tests "passed without the fix", which would have been a false all-clear. With the delete genuinely removed, two of the three fail.
