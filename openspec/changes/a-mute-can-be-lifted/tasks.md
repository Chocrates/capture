## 1. Make the three predicates agree

The live defect, and first because `unmute` would otherwise inherit it.

- [x] 1.1 Write a failing test: a note muted with an end, read after that end has
      passed, is eligible for review. Through `capture_store::mutate::mute_scope`
      with `Some(until)`, since no command can set one yet.
- [x] 1.2 Fix `review_eligible()` (projection.rs:733) to ask
      `muted_until IS NULL OR (muted_until != 0 AND muted_until <= ?1)`. `?1` is
      already `now` for the backoff ladder, so no signature changes.
- [x] 1.3 Give `distinct_mentions` a `now: u64` parameter and the same predicate
      in both branches. Update its four call sites: `ipc.rs:866` and three in
      `crates/capture-store/tests/links.rs`.
- [x] 1.4 Test the tag index the same way. Done in
      `crates/capture-store/tests/links.rs`, where `distinct_mentions` lives,
      rather than through `ipc::wiki_index` — the desktop test covers that
      function's pages branch and its tags branch stays uncovered, which is
      noted as follow-up rather than quietly claimed.
- [x] 1.5 Sabotage both fixes and confirm each test fails.

## 2. An end can be stated

- [x] 2.1 Add `until: Option<u64>` to `Command::Mute`, and `until <YYYY-MM-DD>`
      parsing shared by every target form. Resolve to the start of that day in
      the local zone via `capture_time::at_local`.
- [x] 2.2 Refuse a date already past, naming `capture unmute` as what was meant.
- [x] 2.3 Stop `main.rs` passing a hardcoded `None`.
- [x] 2.4 Unit-test the parsing in `args.rs` beside the existing `mute` cases:
      every target form with and without `until`, a past date, a malformed date.
- [x] 2.5 Test the lapse end to end with `CAPTURE_NOW_MS`: muted before the
      date, present after it, with no command in between.

## 3. A mute can be lifted

- [x] 3.1 Add `write::unmute(paths, &Muted)`, writing a `MUTE` whose `until` is
      the present instant, through `mute_scope` for note, tag and date-range
      scopes and `mute_entity` for entities.
- [x] 3.2 Refuse unmuting a note that is not muted. Accept the three coarse
      scopes unconditionally and say what was written rather than what changed —
      see the design's reasoning about there being no scope-level record.
- [x] 3.3 Add the `unmute` arm to `args.rs`, accepting the same four target
      forms as `mute`, and the help line beside `capture mute`.
- [x] 3.4 Wire `Command::Unmute` in `main.rs`.

## 4. Tests through the commands

- [x] 4.1 A note is muted and unmuted: absent from search while muted, back in
      search and on every entity page naming it afterwards, and the event count
      grew rather than shrank.
- [x] 4.2 A tag mute is lifted without untagging, and the notes still carry the
      tag — the distinction `fold.rs`'s `untagging_lifts_a_tag_mute` draws.
- [x] 4.3 An entity mute is lifted by a different spelling that reaches the same
      entity, which is what makes it an identity rather than a word.
- [x] 4.4 Unmuting an unmuted note is refused; unmuting an unmuted tag is not.
- [x] 4.5 A note's page reports the end of a dated mute and "indefinitely" for
      one without — already implemented and never reachable, so pinned rather
      than built.
- [x] 4.6 Sabotage every test above and confirm each fails. Two tests written
      earlier today passed until sabotaged, so this is not optional.

## 5. Gates and close-out

- [x] 5.1 `make ci`. `deny` cannot reach github.com from here; everything else
      must pass.
- [x] 5.2 Check `tools/unwired.py` and `tools/unreached.py` are quiet — the
      whole defect was a mechanism with no caller, so a new one left half-wired
      would be the same mistake.
- [ ] 5.3 Record in `~/.local/share/capture-reviews/PROGRESS.md`: the ledger
      entry that opened this says it was deferred, and should say what happened.
- [ ] 5.4 Note the two follow-ups in the ledger rather than losing them: the ten
      duplicated `muted_until` predicates, and the desktop and phone controls
      §2.6's "from any device" still wants.
- [ ] 5.5 `openspec archive a-mute-can-be-lifted`.

## 6. Found on the way, not fixed here

- [ ] 6.1 **`capture mute #tag` misses every note that tagged itself inline.**
      A `#noise` written in a capture becomes a `#`-sigil *mention*;
      `apply_mutes`'s `Tag` arm matches on the `tags` table, which only
      `capture tag` writes. So the normal way to tag something — writing it in
      the capture, per §3.1 — is the way muting that tag does not cover.
      `distinct_mentions` already unions both sources, so the system treats
      them as one tag everywhere else. Pre-existing, and a fold change rather
      than a surface one.
- [ ] 6.2 `said` printed `muted until 1791295200000` — fixed here, because the
      syntax added here is what made the branch reachable. Noted so the
      sequence is not mistaken for an unrelated tidy-up.
