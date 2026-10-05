# Tasks

## 1. One rule

- [x] 1.1 `syncstate::peer_is_stale(last_seen: Option<u64>, now: u64) -> bool`,
      with the span as a named constant beside it and the reasoning for the
      week written where the rule is.
- [x] 1.2 The doc comment says why "never reached" counts as quiet, since that
      is the judgement a second copy would most likely get different.

## 2. Both surfaces call it

- [x] 2.1 The terminal drops its constant and its predicate.
- [x] 2.2 The window drops its constant and its predicate.
- [x] 2.3 Neither changes what it prints. The shared thing is the decision, not
      the sentence.

## 3. The boundary, which nothing tested

- [x] 3.1 Inside the span is not quiet.
- [x] 3.2 Outside the span is quiet.
- [x] 3.3 Exactly the span — pin which side it falls on, rather than leaving it
      to whoever reads the `>` next.
- [x] 3.4 Never reached is quiet.

## 4. Guards

- [x] 4.1 Non-vacuity: each test fails against a plausible wrong rule, not just
      a typo. Three tried, all caught: `>=` for `>` (the boundary test), and
      `is_some_and` for `is_none_or` (never-reached), and plain subtraction for
      `saturating_sub` — the last of which panics on a clock that went
      backwards, which is why that case was worth a test of its own.
- [x] 4.2 `make ci`. 80 suites green; `deny` fails on network only.
- [x] 4.3 Record what `duplicated.py` cannot see here, and why widening it would
      fire on the two unrelated weeks in the tree. Recorded in the ledger: the
      gate compares function *bodies*, and this was a constant plus an inline
      expression. Widening it to constants would fire on
      `notifier::STALE_MS` and on `read.rs`'s `AHEAD_MS`, which are a week each
      and genuinely unrelated — so the honest version is a gate that cannot see
      everything and a note saying which things.
