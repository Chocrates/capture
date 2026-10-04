# Tasks

## 1. Pin the defect before fixing it

- [x] 1.1 Add a failing test in `crates/capture-parse/tests/recognize.rs`:
      `dentist thursday 2pm >fri` carries Friday and not Thursday. Confirm it
      fails on the current code, and fails by naming Thursday rather than by
      panicking, so it is testing the defect and not a typo.
- [x] 1.2 Add a failing test that `meeting >fri` is dated at all.

## 2. Read a day reference behind the sigil

- [x] 2.1 Add a resolver in `crates/capture-parse/src/recognize.rs` taking the
      marker value, `now` and `zone`: a full `YYYY-MM-DD` first, then a weekday
      matched against `WEEKDAYS` by whole-word equality and resolved by
      `next_weekday`, then a day word resolved by `day_word`. It must call
      those two rather than re-derive either.
- [x] 2.2 Use it in place of `markers.date.as_deref().and_then(FloatingDate::parse)`,
      keeping `confidence: 100` and the existing task/event discrimination from
      the head verb.
- [x] 2.3 Confirm 1.1 and 1.2 now pass.

## 3. An unreadable marker suppresses the inference

- [x] 3.1 Where `markers.date` is `Some` and does not resolve, do not consult
      the temporal tier; fall through to the verb-or-nothing arm.
- [x] 3.2 Test `review the draft thursday >asdf`: no date, and Thursday absent.
- [x] 3.3 Test `launch the >satellite`: no date. This is the one `weekday_bit`
      would have got wrong, so it is the test that proves the vocabulary is
      matched as whole words.
- [x] 3.4 Test `plan the offsite >q2`: no date.
- [ ] 3.5 Test that the capture's text is unchanged in every case above
      (§3.1.3), not merely that the shape is undated.

## 4. Keep the two readings from drifting

- [x] 4.1 Test that `>friday` and a bare `friday`, recognised at the same
      instant in the same zone, resolve to the same date. This is the test that
      fails if somebody later gives the sigil its own weekday rule.
- [x] 4.2 Test the existing ISO spellings still behave: `pay the invoice
      >2026-09-20` is a task due that day, `dentist >2026-09-20` is an event.

## 5. The grammar and the guards

- [x] 5.1 Check whether `GRAMMAR_VERSION` must rise. The published EBNF already
      promised `day_ref`, so this is an implementation catching up with a
      grammar rather than a grammar change — decide deliberately and record
      which it is, because the version is what a peer uses to decide whether to
      re-parse.
- [x] 5.2 Run `make duplicated` and confirm the resolver did not become a second
      copy of the weekday rule.
- [x] 5.3 Run `make ci`.

## 6. Verify against the real recognizer

- [ ] 6.1 Run the recognizer on the spec's own examples and paste the outputs
      into the archived change: `>fri` against "thursday", `>tomorrow`,
      `>satellite`, `>q2`, `>asdf`, and the two ISO cases. The defect was found
      by reading and confirmed by running; the fix is confirmed the same way.
- [ ] 6.2 Capture one `>fri` on the phone and confirm the promotion reads
      Friday, against `dev.capture.dev` only — never the real archive.
