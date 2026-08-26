## 1. brain-time — the durable layer, nothing depends on it yet

- [x] 1.1 Create `crates/brain-time`, `no_std`, no dependency that can reach a clock. Every entry point takes `now` as a value; the crate must not be able to name a clock, for the same reason `brain-core` cannot (`DESIGN.md` §8.3, D12).
- [x] 1.2 Implement the two time kinds: an instant as UTC milliseconds plus its IANA zone id, and a floating date as `YYYY-MM-DD` with no zone and no conversion path. Make converting a floating date to an instant impossible to express rather than merely discouraged — it is the single most common calendar bug in the wild.
- [x] 1.3 Implement the RFC 5545 subset: `FREQ`, `INTERVAL`, `BYDAY`, `BYMONTHDAY`, `COUNT`, `UNTIL`. Accept unbounded series ("get up at 7" has no end date).
- [x] 1.4 Implement expansion in local wall time, converting to instants afterwards. **Property-tested**: expanding the same rule with the same `now` twice yields identical occurrences, and expansion is a pure function of its inputs.
- [x] 1.5 Implement the DST policy as fixtures rather than reasoning: a nonexistent local time shifts forward to the first valid instant, an ambiguous one takes the first, and both record on the occurrence that it happened. §4.4 draws a marker from this — a silent shift is what makes someone distrust the whole system. **Done**: the gap is found by walking forward a minute at a time rather than assuming an hour — `Australia/Lord_Howe` shifts by 30 minutes and is in the property tests for that reason.
- [x] 1.6 Add the timezone database dependency and confirm it does not drag in a clock. **Purity gate**: extend `make purity` to cover `brain-time` the way it covers `brain-core`.

## 2. Event kinds — permanent from their first write

- [ ] 2.1 Add `promote`, `intent`, `review`, `occ_move`, `occ_edit`, `occ_cancel`, `occ_done`, `occ_undone` to `Kind`, continuing from `UNFLAG(15)`. Numbers are permanent.
- [ ] 2.2 Define each body with its nonce at field index 0, like every other body — the redaction commitment depends on it (§1).
- [ ] 2.3 Write golden encoding vectors for every new body **before any code writes one**. The encoding is frozen from first write; a field forgotten here cannot be added later.
- [ ] 2.4 Extend the round-trip property test to the new kinds: encode, decode, recompute the id, byte-for-byte identical.
- [ ] 2.5 Confirm an older build **holds** an unknown new kind with a recorded reason rather than discarding it, and that a held event is reported.

## 3. Fold and conflict rules — property-tested, not example-tested

- [ ] 3.1 Fold `promote` into the note: derived type, fields, and the regenerated one-line descriptor. The captured text is never touched (§3.1.3).
- [ ] 3.2 Implement promotion conflict resolution: LWW by HLC **among equal authority only**, and a human `promote` (`pinned: true`) beating a parser-authored one regardless of HLC. **Property-tested** — this is the one rule in the system that is not plain LWW, and the failure it prevents is a reparse silently reverting a correction.
- [ ] 3.3 Fold the occurrence exceptions, keyed by `(tgt, occ)`. `occ_done` idempotent; `occ_undone` able to supersede it.
- [ ] 3.4 Implement `occ_move` beating `occ_cancel` on the same occurrence, with the cancel retained and inert. A resurrected occurrence is visible and one gesture away; a wrongly cancelled one is invisible and silently missed.
- [ ] 3.5 Extend the convergence property test to every new kind: shuffle any event set, fold it, and the result must be identical.
- [ ] 3.6 Extend the incremental-equals-full-rebuild property test to cover promotions and occurrence exceptions.

## 4. The recognizer — a parser over the tables that already exist

- [ ] 4.1 Implement the tokenizer over the closed vocabulary in `brain-parse`. A word outside `CLASSES` is not matched approximately.
- [ ] 4.2 Implement tier 1: the six sigils. A sigil always beats inference and suppresses the competing inference entirely — not even a suggestion (§3.1.1).
- [ ] 4.3 Implement tier 2 over the EBNF in §3.1.5: `point`, `range`, `recurrence`, and the `clock` and `anchor` productions.
- [ ] 4.4 Implement the guards, which are not part of the grammar and matter more than it: an article kills a match, a bare number is never a time, a following noun kills a bare weekday or month, and a match must start at the head, at a comma, or after a marker. **Each of §3.1.3's own examples is a test**: "the march timeline slipped", "the 7am problem", "budget 40k".
- [ ] 4.5 Implement confidence scoring against the bands in `BANDS`, and return a recognition rather than an event — the parser stays a pure function of `(text, now, zone)`.
- [ ] 4.6 Test exhaustively over the closed vocabulary rather than representatively. The vocabulary is closed precisely so this is possible.
- [ ] 4.7 Check the five worked examples in §3.1 parse to the types the document claims, and record any that do not — the vocabulary was written without a parser to test it against.
- [ ] 4.8 Add a grammar version constant to `brain-parse`, bumped whenever the vocabulary, the grammar or the bands change. This is the reparse trigger: nothing in the system observes a build, so without it a note captured in March can never benefit from a fix shipped in June.
- [ ] 4.9 Treat the confidence bands as provisional and strict. A band moves only when a real capture forces it, and that capture is pinned alongside as a test case — so the bands accumulate evidence in the direction §3.1.3 prefers, where a miss costs a glance and a false positive costs a calendar entry.

## 5. Promotion — where recognitions become events

- [ ] 5.1 Author `promote` from a recognition at or above the 0.75 floor. Enforce the floor in one place.
- [ ] 5.2 Record recognitions below the floor as suggestions, surfaced as one line in review, applied to nothing.
- [ ] 5.3 Implement the reparse path on text edit, respecting pins.
- [ ] 5.3a Record the grammar version on every promotion, and sweep at startup: anything promoted under an older version is reparsed, pins untouched. The cost is the one §2.5.1 already anticipates — one `promote` per note per sweep.
- [ ] 5.4 Implement the human correction path, authoring `pinned: true`.
- [ ] 5.5 Implement tasks: state, optional floating due date, list membership as tags with a display order, optional ordering.
- [ ] 5.6 Implement recurring obligations, with uncompleted instances recorded as lapsed and **not** rolling over, stacking or accumulating.
- [ ] 5.7 Implement `carries_forward`, defaulting to false and never set by the recognizer. A series that has lapsed repeatedly prompts once, in review. This is the foot-gun exception: a missed dose is not the same as a missed walk.
- [ ] 5.8 Confirm parsing never delays capture — the overlay closes on Enter with no parser present, and the latency histogram is unchanged.

## 6. Projection — occurrences and the widened queue

- [ ] 6.1 Expand occurrences into `state.db` over the rolling horizon of −1 year / +1 year (§3.4), extended in the background and on view access.
- [ ] 6.1a Measure what the horizon actually costs — rows and rebuild time for a dozen unbounded daily series. It is a performance choice, not a correctness one (failure mode 23), and it was kept rather than re-guessed, so it should be measured once rather than argued about again.
- [ ] 6.2 Store promoted fields and descriptors so agenda and calendar queries never parse text at read time.
- [ ] 6.3 Implement the series split for "this and future": bound with `UNTIL`, new series, `split_from`, and exceptions binding to whichever series covers their `original_start`.
- [ ] 6.4 Widen the due-work queue to carry review and intent deadlines beside redaction deadlines, processed by the same startup-is-the-floor mechanism.
- [ ] 6.4a Drain the due queue from the resident overlay on a timer. It runs all day for the chord and is already the elected writer (§8.8), but it never opens a `Session` — so today a machine with the overlay running and nothing else touching the store leaves due work undone until some command happens to run. Startup stays the floor; this is what makes a deadline land near its time without adding a process that can be silently not running.
- [ ] 6.5 Confirm a rebuild reproduces every expansion exactly, and that a timezone database change is fixed by re-folding rather than by migration.

## 7. Surfaces — the desktop first, it is the first-party one

- [ ] 7.1 Note page: the captured text and the current fields side by side, so drift is visible rather than silent.
- [ ] 7.2 Note page: suggestions below the floor offered as one line, applied to nothing.
- [ ] 7.3 Calendar: all-day events in a band above the timed grid, timed occurrences column-packed, DST-shifted occurrences marked, cancelled ones hidden but recoverable.
- [ ] 7.4 Agenda: what is due and what occurs, drawn from the same substrate — a meeting and the notes taken during it are one record, not two.
- [ ] 7.5 Occurrence completion in the GUI, including un-completing a mistaken tick.
- [ ] 7.6 CLI equivalents: agenda, tasks, occurrence completion, and the pinned correction path. Every one takes `--json`.

## 8. Review — the component with the best evidence

- [ ] 8.1 Implement the session at **five items**. The property that matters is that it is always finishable — a session you do not finish is a backlog by another name. Not a setting: a knob nobody turns is a default with extra code.
- [ ] 8.2 Implement selection in the stated priority order: forward markers, active project or upcoming event, never engaged since capture (oldest first), matching ambient context.
- [ ] 8.3 Implement the session shape: bounded, ending, and saying so. Never an unbounded queue — §3.6 spends its length on why manufactured backlogs are the wrong shape here.
- [ ] 8.4 Implement the `review` event with outcomes `shown / engaged / deferred` only.
- [ ] 8.5 Implement scheduling by engagement rather than accuracy: engaged recurs less often, deferred twice stops being offered rather than being pressed.
- [ ] 8.6 Confirm by test that **no score, streak, accuracy or aggregate exists anywhere, at any aggregation**. This is the surface where a graded record would do the most harm, and the absence is a requirement rather than an omission.
- [ ] 8.7 Confirm `mute` applies to review, and that "pause review" and "less often" work without asking for a reason.

## 9. Intents — if-then plans firing locally

- [ ] 9.1 Implement the `intent` event: cue specification plus action.
- [ ] 9.2 Implement the cue types this device can genuinely observe: `time`, `recurring`, `device_wake`, `app_focus`, `git_branch`. Declaring a cue type requires being able to observe it.
- [ ] 9.3 Fire local cues where they are observed — `app_focus` and `git_branch` on the machine running the application, never redirected to a designated notifier.
- [ ] 9.4 Implement notifier designation as a `setting`, and the compiled-in default as a **rule evaluated locally and never written to the log**. A machine-authored preference recorded as a user decision is what §8.9 forbids.
- [ ] 9.5 Confirm `brain status` reports the notifier as a default rather than as a choice until the user makes one.

## 10. Activation

- [ ] 10.1 Implement a context surfacing the notes captured against it, using the recurrence machinery and one event kind.
- [ ] 10.2 Implement external calendar import as read-only into a shadow namespace: visible in calendar and agenda, available as ambient context, never authored into the log, never written back.
- [ ] 10.3 Confirm notes survive their external calendar disappearing, with captured context intact, and that stale shadow entries are marked stale rather than silently dropped.

## 11. Acceptance

- [ ] 11.1 Extend the acceptance suite: each spec scenario in this change has a runnable test.
- [ ] 11.2 Confirm the M1 capture gesture is unchanged — chord, overlay, commit key — and that the capture latency histogram is unaffected by anything here (§8.10 freezes it for the duration).
- [ ] 11.3 Run the recognizer over the real captures accumulated during M1 use and record what it gets wrong. The confidence bands in §3.1.5 are reasoned rather than measured, and this is the only honest sample.
- [ ] 11.4 Confirm `make ci` passes, including `deny` — which requires `cargo-deny` to actually be installed, and has never run on this machine.
