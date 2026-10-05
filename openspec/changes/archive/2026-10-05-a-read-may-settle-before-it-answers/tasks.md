# Tasks

## 1. The requirement

- [x] 1.1 Say that a read may take the lock to *resolve* staleness, never to
      discover it, and that it reports rather than fails when it cannot.
- [x] 1.2 Carry the reason, so the sentence is not re-argued on its own and
      "fixed" back — the weak reading shipped once and answered from stale
      state.
- [x] 1.3 Scenarios for both halves: `due` settles first, and says so when the
      lock is held.

## 2. Check the code already matches

- [x] 2.1 Confirm `due` settles before answering, by running it rather than
      reading it. On a scratch store with one unread capture: `unread = 1`
      before, `capture due` answers with the task, `unread = 0` after.
- [x] 2.2 Confirm it answers and reports when the lock is held, which is the
      half that would be easy to lose. Holding `writer.lock` with `flock` and
      asking:

      ```
        waiting       Tue 00:00  pay the invoice tomorrow
      1 coming, none due yet.

      Captures could not be read first: the store is being written by 1721236 cli
      So this may be missing something. Try again once that finishes.
      ```

      It answers from what it has, says what it could not do, and names the
      process — which is the behaviour the requirement now describes, rather
      than the one the old sentence described.
- [x] 2.3 `make ci`. 83 suites green; `deny` fails on network only. No code
      changed — this is the document catching up.
