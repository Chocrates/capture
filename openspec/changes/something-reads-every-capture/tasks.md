## 1. The operation

- [x] 1.1 `promote::settle(paths) -> Result<usize>`. One place for the comment explaining why anything has to call it.
- [x] 1.2 Contention is an error, not a zero. `Session::start` fails fast when the lock is held and `settle` passes that through.
- [x] 1.3 **The count needed a session field, not a second sweep.** Opening *is* the sweep — `catch_up` already ran it — so sweeping again after opening read the whole backlog twice and then reported zero, the first pass having promoted all of it. Wrong in the direction that hides the bug. `Session::read_at_open` records it and `settle` reports that. Pinned by a test.

## 2. Detecting it read-only

- [x] 2.1 `Store::unread_count(grammar)`, sharing `UNREAD_FROM_WHERE` with `needing_parse` rather than restating the predicate — two copies drift, and the drift reads as either "nothing outstanding" over a backlog or a warning nothing clears.
- [x] 2.2 Asserted with the writer lock held, which is the property it exists for (§8.8).
- [x] 2.3 Zero after a settle, non-zero after a capture, non-zero again after an edit. **An empty store cannot be read at all** — `projection::current` needs `ops.db`, which does not exist until something opens the store — so that case is not asserted; it is pre-existing and not this change's business.

## 3. The queue was infinite, and nothing worked until it was not

Not in the plan. Found by 2.3 asking whether the staleness line ever clears.

- [x] 3.1 **`read_by` table**: that a note was read, by which grammar, at which text position — written for every note the sweep looks at, *including the ones it promotes to nothing*. Nothing recorded a negative result before, so `needing_parse` read a missing promotion as "not yet read" and every ordinary capture stayed queued for ever.
- [x] 3.2 A table and not a column on `notes`. `projection.rs:118` states the rule and what it cost to learn; `parents` is a table for the same reason.
- [x] 3.3 `PROJECTION_SCHEMA` 9 → 10 with its changelog entry. Nothing to migrate — it records what *this build* has looked at, so empty is the honest starting state.
- [x] 3.4 **The cost, measured:** one dated task, 250 ordinary notes, five sweeps — `0 promoted, 251 queued`, and the task captured *first* was never read at all. `ORDER BY created_wall DESC LIMIT 200` over a queue that never drains is not a bound on work per run, it is a horizon on the archive: any archive past a couple of hundred notes had a sweep doing nothing and a grammar improvement reaching nothing.
- [x] 3.5 Both halves asserted: the queue empties for an ordinary note, and the oldest capture is reached rather than starved behind the newest 200.
- [x] 3.6 **The first version of the predicate would have corrupted the real archive, and a test caught it before commit.** `read_by` is local and a rebuild drops it, while `promotions` is re-folded from the log — so immediately after the 9 → 10 rebuild every already-promoted note has a current promotion and no read record. Reading a missing `read_by` row alone as "unread" meant the first sweep after this change would re-promote the entire archive, writing one duplicate promote event per note saying exactly what the existing one said, and syncing all of them to every peer. The same held on any second device, where a promotion arrives by sync and no local read was ever recorded. Fixed by making a *current promotion* count as a record of having been read, symmetrically with `read_by`; asserted by comparing the log's event count across a simulated rebuild.

## 4. The daemon

- [x] 4.1 Settle on `capture-syncd`'s loop, before `tick`, so it no longer depends on having a peer. `round::tick` gives up before `reconcile` on no roster secret, no members, or nothing reachable — so promotion used to be a side effect of *having somebody to sync with*.
- [x] 4.2 A failed settle is logged at debug and the round continues. The lock is held by whoever is capturing; the next round finds it free.
- [x] 4.3 Covered by the store tests rather than by driving the binary: every `Scratch` has no roster and no peers, which is exactly the case that was broken.
- [x] 4.4 **Measured, release, 250 captures:** first pass (200 read) 2.86s, second (50 read) 753ms, nothing-to-do 27ms. ~14ms a note, and the steady state the daemon actually sees is 27ms. `PER_SWEEP` stays at 200 — the pass is long but happens once per archive, and a capture during it spools rather than failing. Kept as an ignored test with the number written down.

## 5. The command line

- [x] 5.1 `capture settle`, reporting the count, "nothing was waiting", or who holds the lock.
- [x] 5.2 `say_if_behind` on `tasks` and `agenda`, both branches of each — one line after the answer, naming `capture settle`.
- [x] 5.3 `tasks --json` gains `unread`. It is now an object rather than a bare array: a consumer reading an empty list as "no tasks" was being told something that might not be true.
- [x] 5.4 **`cli::due` stops swallowing its sweep.** It was `let swept = Session::open(…); drop(swept)` — the `Result` thrown away, so a contended store gave no sweep, no error and an answer from stale state. In the command whose whole purpose is disbelieving "nothing is due". Now `settle` with the outcome reported, and a failure says who has the store.
- [x] 5.5 The transcript from the proposal, reproduced on a scratch `CAPTURE_HOME`: capture → `tasks` says "No tasks" *and admits it has not read 1 capture* → `settle` → the task is there → the line is gone.
- [x] 5.6 `settle` arg parsing tested, including that it takes nothing but `--json`.

## 6. Android

- [x] 6.1 **Already done, and the design said otherwise.** `Archive::settle()` exists, its doc says it is called when the application comes to the foreground, and `MainActivity.kt:119` calls it. It opens a session, which sweeps. The hole was only ever on the desktop. Design corrected rather than quietly dropped.
- [x] 6.2 `ffi::due` calls `settle()` instead of reimplementing it inline. It keeps its sweep — the alarm receiver arrives in the background with no guarantee the foreground pass has run — but as one named operation.
- [x] 6.3 `Archive::unread()`, read-only, so a surface can report being behind without becoming a writer.
- [x] 6.4 `make android-decides` clean: 22 Kotlin files, none deciding anything.
- [ ] 6.5 Device case: capture a dated task, go to Tasks, see it — against `dev.capture.dev`, never the real archive. Needs the phone.
- [ ] 6.6 Device case: capture, background, wait for the alarm, confirm `due` still fires. The path that already worked must keep working.

## 7. Open after this

- [ ] 7.1 The daemon's settle is not bounded by the round's `spent` budget — `BLOB_WINDOW_MS` is computed before it. A 2.86s catch-up overruns a round that thought it had a window. Worth deciding whether the sweep should take the same budget.
- [ ] 7.2 No staleness surface on the phone. `unread()` exists and nothing calls it; the foreground settle means it should almost never be non-zero, which argues both ways.
