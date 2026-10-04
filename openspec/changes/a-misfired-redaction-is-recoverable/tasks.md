## 1. Seeing what is there

- [x] 1.1 `Ops::snapshots(now)` and a `Snapshot` type — which redactions still have a live copy, when taken, when they expire, and the text. `snapshot_count` answered how many and nothing answered *which*, so the copies could be reported and never chosen.

## 2. The command

- [x] 2.1 `capture recover` lists them, oldest first — the one about to expire is the one a decision is most urgent about — with the first line of each and how long it has left.
- [x] 2.2 `capture recover <id>`, named by the same prefix `redact --cancel` takes, so the recovery routes inside and outside the window are reached the same way.
- [x] 2.3 It says a new note was written, never that the redaction was undone. That would be the more dangerous of the two available lies: somebody would stop looking for the other copies.
- [x] 2.4 An unknown or expired id is refused and names both possibilities, because a typo and an expiry are different situations.

## 3. The document

- [x] 3.1 DESIGN said six hours in **five** places; `REDACT_WINDOW_MS` is thirty minutes. A twelve-fold difference in the only safety margin the one irreversible operation has. Corrected in the document rather than the code: the code is what people were relying on, and `capture redact` has always printed the true figure. The constant is named as the authority so the next drift is a one-line fix.

## 4. Seeing it

- [x] 4.1 Capture, redact, drain past the window, and the copy is listed with its text. Before the destruction there is none — the snapshot is taken at purge time, not at redact time, which is worth knowing when reading the test.
- [x] 4.2 The copy outlasts the cancellation window, asserted against `REDACT_WINDOW_MS` rather than a number, since that is the whole argument for keeping one.
- [x] 4.3 A copy past its retention is not offered.
- [x] 4.4 Recovering writes a new note with its own name and the original stays redacted.
- [x] 4.5 The refusal and the empty listing driven on a scratch `CAPTURE_HOME`.

## 5. Found while here, not fixed

- [ ] 5.1 **The mass-redaction ceiling is a no-op.** §1.1 promises 256 targets per hour per origin, held for confirmation beyond that. `redaction_history()` queries `note_versions` for `Kind::REDACT`, and that table only ever receives CAPTURE, EDIT and TOMBSTONE — so it returns zero rows always, `ratelimit::check` always says `Proceed`, and `Drained.held` can never be non-zero. The limiter itself is correct and well tested; the query feeding it is not. It is also not per-origin and would count redactions rather than targets. Its own milestone task is marked done. Needs its own change.
