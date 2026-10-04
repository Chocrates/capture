# Tasks

## 1. The store

- [x] 1.1 `mutate::move_occurrence(paths, name, occ, start_ms, end_ms)` writing
      `OCC_MOVE` with an `OccMoveBody`.
- [x] 1.2 `mutate::amend_occurrence(paths, name, occ, title, location, notes)`
      writing `OCC_EDIT` with an `OccEditBody` — the first writer of that kind
      anywhere.
- [x] 1.3 Both refuse a note with no occurrences through the existing `series()`
      helper, so there is one refusal rather than three.
- [x] 1.4 `restore_occurrence` keeps its own body construction. Collapsing it
      into a general move would make the call site read "move this to where it
      already is" and lose the reason its comment records.

## 2. The command line

- [x] 2.1 `capture move <series> <occ> <start-ms> [<end-ms>]`.
- [x] 2.2 `capture amend <series> <occ> [--title T] [--location L] [--notes N]`,
      refusing when none of the three is given — an amendment that amends
      nothing should say so rather than write an empty exception.
- [x] 2.3 Usage text and arg tests, including both refusals.

## 3. Test what was built and never reachable

- [x] 3.1 A moved occurrence shows at its new time; every sibling and the rule
      are untouched.
- [x] 3.2 A completion written *before* a move still applies after it — the
      point of naming an occurrence by its original start.
- [x] 3.3 An amended occurrence carries its own title while its siblings carry
      the series'.
- [x] 3.4 Amending does not move anything, and moving does not retitle anything.
- [ ] 3.5 An exception survives a split, binding to whichever series covers its
      original start. The fold implements this deliberately; nothing has ever
      exercised it through a real amendment.

## 4. The DST case, which turned out not to be the case

- [x] 4.1 Move an occurrence onto an ambiguous hour and see what the column
      says. **It says nothing**, and the premise of this section was wrong:
      `shift` is filled from the expansion's own value and the fallback path
      writes `NULL`, so neither recomputes it for the destination.
- [x] 4.2 Decide deliberately rather than by default. Left as it is: §4.4's
      argument on that column is about a time moving *on its own*, and a move
      is the person choosing. Whether a calendar should still warn that a wall
      time happens twice is a real question and a separate change.
- [x] 4.3 Assert today's answer, so implementing it later is a deliberate edit
      to a test that states the old one — `a_move_onto_a_dst_transition_is_not_marked`,
      whose failure message says what to do if it ever starts passing the other
      way.

## 5. Guards

- [ ] 5.1 Verify non-vacuity: each new test fails with its fix reverted.
- [x] 5.2 `make ci`. 78 suites green; `deny` fails on network only — it cannot
      reach the RustSec advisory database from this sandbox.
- [ ] 5.3 End to end on a **scratch** `CAPTURE_HOME`, never the real archive:
      `standup every weekday at 9`, move Tuesday to 14:00, confirm the agenda
      shows it there and Monday is untouched; amend Wednesday's title and
      confirm Thursday still reads the series'.
