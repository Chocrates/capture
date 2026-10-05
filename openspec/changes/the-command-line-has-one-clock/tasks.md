# Tasks

## 1. One clock

- [x] 1.1 `now_ms()` honours `CAPTURE_NOW_MS` under `cfg(debug_assertions)`,
      with the reasoning for the guard written where the guard is.
- [x] 1.2 Every one of the fourteen direct clock reads goes through it,
      including `backup.rs`'s filename timestamp, which needs a `Timestamp`
      rather than milliseconds.
- [x] 1.3 A check that none is left behind: grep for `Timestamp::now` in this
      binary and expect exactly one.

## 2. What it actually reaches

- [x] 2.1 Establish by running it, not by reading: does the override move the
      drain? **No** — a capture made with it set to 2030 lands on today's
      timeline, because the store stamps events with its own clock.
- [x] 2.2 Correct the proposal, the design and the doc comment, all of which
      claimed this unblocked `capture recover`.
- [x] 2.3 A test over something it *does* reach, so the hook is not shipped
      unexercised.

## 3. Guards

- [x] 3.1 Confirm a release build ignores the variable. By construction: the
      block is behind `cfg(debug_assertions)`, so the code reading it is not
      compiled into a release.
- [x] 3.2 Non-vacuity. The recover test was not written — see 2.1, this does
      not reach the drain. Instead: compiling the override out fails
      `a_task_is_overdue_or_not_depending_on_when_you_ask` with "a task due
      yesterday is not: due", so the hook is exercised rather than shipped on
      trust.
- [x] 3.3 `make ci`. 83 suites green; `deny` fails on network only.
