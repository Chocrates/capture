## 1. Recognizer

- [x] 1.1 Tests first: weekday+date agree (four spellings) → that date, with the time; disagree → unclear day with both choices; "next thursday" on Tue → unclear (15/22); on Fri → coming; on Thu → +7; "this thursday" on Thu → today; "last thursday" → previous.
- [x] 1.2 `FloatingDate::weekday`, `plus_days` in capture-time.
- [x] 1.3 `point()`: weekday/date pairing both orders; determiners per D2; `UnclearDay`; grammar 9.
- [x] 1.4 `unclear_day(text, now, zone)`, `answer_day(text, now, zone, which)` (D3).

## 2. Store

- [x] 2.1 Tests first: an unclear day appears on both days in `calendar::days` and nowhere else; settled → one dated item; pinned → not asked.
- [x] 2.2 Projection `unclear_days` (schema 16); `calendar::Item.unclear_day`.
- [x] 2.3 `mutate::settle_day`; `promote::reread`; review line for an unclear day.
- [x] 2.4 `suggestion_for` wording (D5), with tests for the therapy text.

## 3. Surfaces

- [x] 3.1 CLI: `capture day NAME 1|2`, `capture reread NAME`, `show` and `cal` name the question.
- [x] 3.2 FFI: `unclear_day`, `settle_day`, `reread`; `AgendaEntry.unclear_day`.
- [~] 3.3 Phone: calendar row and note page ask; note page "Read it again" where a correction is pinned.
  - Builds; FFI tested. Not yet seen on the device.
- [x] 3.4 Desktop: the same, IPC-tested.

## 4. Verification

- [x] 4.1 `make ci`; sabotage each fix once.
- [x] 4.2 Phone dev app: "therapy next thursday at 4pm" shows on both Thursdays with the question; answering lands it.
  - Seen on the dev app 2026-10-13 (ledger).
- [ ] 4.3 Real use: the person's therapy note is re-read and lands on 22 Oct 16:00.

## Notes from building it

- Undoing a correction needed a fold change, not just a store function: a pin outranked every
  later promotion for ever, by design. A *release* (`PromoteBody.released`, a new trailing optional
  field, so existing events are byte-identical) is a person's promotion that is not a pin. The fold
  keeps the person's latest and the parser's latest as two registers and derives the promotion from
  both, so every arrival order agrees (test: `a_person_can_take_a_correction_back_in_any_arrival_order`).
  A device on an older build ignores the field and keeps the pin until it updates.
- Moving an occurrence "to next thursday" is refused with the same question, as a bare hour is.
- The "Monday 12 has no gym" report was not a bug: that series had one occurrence moved and was
  then deleted; the gym re-entered on Tuesday starts Tuesday. Kept the regression test
  `a_series_written_on_its_own_day_has_that_day`.
