## 1. The store

- [x] 1.1 `mutate::skip_occurrence(paths, name, occ)` writing `OCC_CANCEL` with an `OccBody`.
- [x] 1.2 `mutate::restore_occurrence(paths, name, occ)` writing `OCC_MOVE` back to `occ`.
- [x] 1.3 Refuses a note with no occurrences, naming what produces them, rather than writing an event that changes nothing when folded.
- [x] 1.4 In `capture-store` rather than beside the command line's `occurrence`, because §8.4 wants every mutation reachable from the window too and a second implementation there is how two surfaces come to disagree. `occurrence` should follow; that is a separate tidy.

## 2. The command line

- [x] 2.1 `capture skip <series> <occ>` / `capture unskip`, naming the occurrence exactly as `capture done` does.
- [x] 2.2 Usage text, and an arg test including the refusal when no occurrence is given.

## 3. The bug this found

- [x] 3.1 **`OccState::visible()` was read by nothing but its own test.** It says a move beats a cancel on the same occurrence — §6's reasoning, implemented by m2's task 3.4 — and `occurrences_of`'s own comment claims the behaviour. But the expansion wrote `cancelled` straight through and ignored `moved_to`, and that column is what every surface filters on. So the rule was true in the fold and absent everywhere it mattered, and the designed recovery gesture did nothing at all.
- [x] 3.2 Found by *using* it: `unskip` ran, reported success, and the agenda still showed nothing. No amount of reading the fold would have shown this, because the fold is correct.
- [x] 3.3 Fixed in **both** write paths — the expansion and the fallback for exceptions the rule no longer produces. Two paths write that column and they have to agree, or an occurrence changes its mind about being cancelled depending on whether the rule still reaches it.
- [x] 3.4 No `PROJECTION_SCHEMA` bump: the state was unreachable until this change, so no existing projection can hold it.
- [x] 3.5 Verified non-vacuous — reverting the one-line fix fails `a_restored_occurrence_is_visible_again`.

## 4. Seeing it

- [x] 4.1 End to end on a scratch `CAPTURE_HOME`: `standup every weekday at 9`, skip Monday the 5th, agenda says "nothing"; Tuesday the 6th still stands; unskip and 09:00 is back.
- [x] 4.2 Asserted that the rule and every other occurrence are untouched, and that no occurrence is renamed — which is what separates this from editing the series.
- [x] 4.3 Both idempotent cases, and that the cancellation stays in the log after a restore rather than being rewritten.
