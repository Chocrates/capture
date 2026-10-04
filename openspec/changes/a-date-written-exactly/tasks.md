## 1. The verb and the sigil

- [x] 1.1 `head_is_verb` moved above the sigil path, which is where the bug was: the sigil returned before the verb was ever looked at.
- [x] 1.2 Both, as tests and end to end through the command line.
- [x] 1.3 Unchanged: the sigil path still returns before tier 2 is consulted. It reads the verb on the way past, which answers a different question.
- [x] 1.4 100 either way.

## 2. A date written in full

- [x] 2.1 **The tokenizer was eating it.** A hyphen between digits is the range operator — `aug 1-14` — so `2026-09-20` was split into three numbers and two operators and never reached the classifier whole. The date is now tried first, which is safe because the shape is exact: ten characters, four-digit year, real calendar day.
- [x] 2.2 Anchored as `CompleteDate` at the same band as `sep 20` — it is exactly as unambiguous and should not read as more so. It also needed adding to the mid-clause guard: a full date needs no permission from what precedes it, which is why `dentist 2026-09-20` now reads and `the 2026-09-20 timeline` still does not.
- [x] 2.3 Those three plus `build 2026-09-201`, which a length check alone would have let through.

## 3. Either way round

- [x] 3.1 Both arms, one anchor, so everything downstream is unchanged.
- [x] 3.2 Asserted as equal shapes rather than equal strings.
- [x] 3.3 Still an offset: the day-first arm needs a month name after the number, and no unit is a month.

## 4. What stays unread

- [x] 4.1 Three forms pinned, including `09/20/2026`. A date guessed wrong is confidently off by months, which is worse than one not read.

## 5. The grammar

- [x] 5.1 4 → 5, with its changelog entry.
- [x] 5.2 Covered by the existing promotion suite, which already asserts a pinned correction survives every later sweep. The new reading is what a reparse now produces.

## 6. Seeing it

- [x] 6.1 On a Pixel 8 Pro against `dev.capture.dev`. `pay the acme invoice on 2026-09-20` — the form that was completely unread — lands in **Overdue** as `task · due 2026-09-20`. All three bands on one screen, plus the series rendering `every weekday at 09:00` on its timeline row and the verb-only capture staying a note until asked.
- [x] 6.2 Over the 614-capture corpus: 30 read as dated, 5 of those as tasks with a due date. Kept as an ignored test beside the near-miss measurements.
