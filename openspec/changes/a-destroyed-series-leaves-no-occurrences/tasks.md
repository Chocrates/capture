# Tasks

## 1. Make the guard able to fail

- [x] 1.1 Widen the fixture in `tests/purge_leaves_nothing.rs` to a recurring
      series with an edited occurrence — a title, a location and notes — so
      `occurrences` holds rows carrying text when the enumeration runs.
- [x] 1.2 Confirm the test now **fails**, naming `occurrences` and a row count.
      If it does not, the fixture is still not reaching the table and nothing
      below is worth doing.
- [x] 1.3 Record what else the widened fixture turns up. Every table it newly
      populates is a table that was previously exempt by accident.

## 2. Emptiness is a failure, not a pass

- [x] 2.1 Count each enumerated table's rows for the target *before* the
      destruction, and fail if a table the test covers held nothing — naming it
      as unexercised rather than reporting it clean.
- [x] 2.2 Add the second allow-list this needs: tables no single capture can
      populate, each with a written reason, in the shape of `MAY_SURVIVE`. Keep
      it separate from `MAY_SURVIVE` — they answer different questions and a
      reason for one must not excuse the other.
- [x] 2.3 Add the stale-reason check for the new list too, matching the one that
      already fails if `MAY_SURVIVE` names a table that no longer exists.

## 2b. Find a table whatever it calls the column

- [x] 2b.1 Enumerate tables holding the note under `src` as well as `tgt`.
      `links` is keyed that way and has never been checked, which is why its
      being cleared by `purge_note` is luck rather than coverage.
- [x] 2b.2 List any table with neither column, each with a reason, so a future
      `note_id` is a failure rather than a silent exemption.
- [x] 2b.3 Verify by regression: remove `purge_note`'s `DELETE FROM links` and
      confirm the guard now fails naming it. Before this task it would not have.
      **It still does not, and that is the finding.** Removing the `links`
      delete changes nothing, because the fold rebuilds a version's links from
      its body and a destroyed note has no body. The same is true of `mentions`
      and `list_items`. Measured for every delete and written up in `design.md`:
      the guard proves "a destroyed note ends with no rows here", not "the purge
      cleared them".

## 3. The delete

- [x] 3.1 Add `DELETE FROM occurrences WHERE tgt = ?1` to `purge_note`, with a
      comment naming what the row carries — `title`, `location` and `notes` come
      from exception bodies the purge has just destroyed.
- [x] 3.2 Fix anything else 1.3 turned up, in the same pass.
- [x] 3.3 Confirm the test passes, then verify it against a regression by
      removing the new delete and checking it fails with the table named.

## 4. The queued window

- [x] 4.1 Write the test first: a series queued for destruction, asked for its
      occurrences. Report what it does before changing anything — `occurrences`
      is expanded from the promotion rather than the text, so it is outside the
      `readable_text` guard, and the agenda may or may not already exclude it.
      **Answer: the rows stay, and they carry no words.** `title`, `location`
      and `notes` are all empty, because nothing can author an `OCC_EDIT`.
- [x] 4.2 If the rows are reachable, withhold them on the same footing as the
      text-derived tables. If they are not, assert that they are not, so the
      safety does not quietly disappear when somebody adds a query.
      Neither, as it turns out: the rows are reachable and hold a *schedule*,
      not words. Withholding them now would be withholding on the strength of
      three columns nothing can fill. So the test asserts the reasoning instead
      — `title.is_none()` for every occurrence — and fails the day
      per-occurrence editing is wired up, which is the day the decision has to
      be made again with something at stake.

## 5. Guards

- [x] 5.1 Run `make ci`. 78 suites green; `deny` fails on network only, as it has
      all session — it cannot reach the RustSec advisory database from this
      sandbox.
- [x] 5.2 Re-read `purge_note` against the full table list one more time, now
      that the guard can fail — and record in the change whether the guard
      agrees with that reading. If the two disagree, the guard is still wrong.

      They agree exactly. `purge_note` now clears twelve note-keyed tables —
      `ambient`, `fts_map`, `links`, `list_items`, `mentions`, `note_fts`,
      `occurrences`, `promotion_at`, `promotions`, `redact_queue`, `tag_adds`,
      `tags` — and the seven it leaves are precisely `MAY_SURVIVE`: `notes`,
      `note_versions`, `parents`, `read_by`, `review`, `late_cancels`,
      `divergences`. No table is in neither set, and none is in both.
