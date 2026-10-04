# Design

## The defect, precisely

`recognize` collects markers from the token stream, where `'>' => m.date =
Some(v.to_string())` keeps the value "exactly as written". The only thing that
then consumes it is

```rust
if let Some(d) = markers.date.as_deref().and_then(FloatingDate::parse) {
```

and `FloatingDate::parse` returns `None` for anything that is not exactly ten
bytes of `YYYY-MM-DD`. On `None` the whole sigil branch is skipped and control
reaches `temporal(&tokens, now, zone, head_is_verb)`, which is told nothing
about the marker and happily reads the prose.

Three consequences, all silent:

1. `dentist thursday 2pm >fri` → Thursday, confidence 85.
2. `meeting >fri` → `Shape::Nothing`, confidence 0 — an explicitly dated capture
   that is not dated.
3. `markers.date = Some("fri")` is still in the returned recognition, so the
   marker is *recorded* while having no effect. Anything downstream reading
   `markers.any()` sees an explicit marker that the shape does not reflect.

## Everything needed already exists

| need | what provides it |
|---|---|
| which Friday `fri` means | `next_weekday` → `capture_time::next_weekday_on_or_after` |
| what `tomorrow` means | `day_word` |
| the closed weekday vocabulary | `WEEKDAYS` (`lib.rs:146`), exact lowercase words |
| `now` and `zone` at the call site | already parameters of `recognize` |

So this is a wiring change plus one decision, not new machinery. That matters
for `duplicated.py`: the resolution must *call* `next_weekday` and `day_word`
rather than re-derive them, because "which Friday" is one decision and a second
copy is free to drift.

## Decision: match the vocabulary as whole words, not by prefix

The obvious shortcut is `weekday_bit`, already in the file:

```rust
let idx = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
    .iter()
    .position(|p| w.starts_with(p))?;
```

It is correct where it is used, because the grammar tier only reaches it for a
token the scanner already classified `Class::Weekday` against the closed list.
Reached directly from a marker value it would read `>satellite` as Saturday,
`>monitor` as Monday and `>sunset` as Sunday. That is approximate matching of a
word outside the vocabulary, which §3.1.5 forbids in as many words.

So the marker value is matched against `WEEKDAYS` by equality, lowercased, and
only then handed to `next_weekday`. The vocabulary stays the one authority on
what a day word is.

## Decision: an unreadable marker suppresses the inference entirely

The alternatives were:

**(a) Ignore it and let the prose win.** The status quo, and the defect.

**(b) Promote with no date.** The marker is an instruction; an instruction that
could not be followed is not permission to substitute a different answer. The
text is kept verbatim either way (§3.1.3), so nothing is lost, and the capture
is still a task if the head verb says so — it simply has no due date.

**(c) Refuse the capture.** Never: capture never fails (§3.1), and a capture
refused for a typo in a marker would be the worst possible trade.

(b). Concretely: when `markers.date` is `Some` and does not resolve, the
temporal tier is not consulted at all, and recognition falls through to the
verb-or-nothing arm. Not consulted rather than consulted-and-filtered, because
the tier returns a shape and a confidence together — a recurrence, a range, a
point — and picking the date out of a shape after the fact is the kind of partial
undo that goes wrong the next time a shape is added.

The cost is honest and small: `pay the invoice >asdf` becomes a task with no due
date at confidence 40, which is below `CONFIDENCE_FLOOR`, so it stays a plain
note with a suggestion. Somebody who mistypes a marker gets their text and no
label, which is exactly what "an honest Note beats a confidently wrong one"
asks for.

## What the tests have to pin

The defect survived because the *negative* halves were never asserted. The
repo contains no occurrence of `>fri`, `>friday` or `>tomorrow` at all, and no
test anywhere pairs a sigil date against a competing prose date in any spelling
— every sigil test uses `>2026-09-20`, where the bug cannot appear.

So the tests that matter here are the disagreement cases and the refusals:

- a sigil weekday against a prose weekday, asserting the prose date is absent
  rather than merely that the sigil's date is present;
- `>friday` and bare `friday` evaluated at the same instant, asserting the
  same date, which is what keeps the two paths from drifting;
- `>satellite`, `>q2`, `>asdf` asserting no date **and** that the prose date did
  not leak in;
- the existing ISO behaviour, unchanged.

## Open questions

1. **Should an unreadable marker produce a near miss?** Telling somebody at
   review time that `>asdf` could not be read is clearly better than silence,
   and the near-miss surface exists. It currently carries one kind of miss —
   the list forms — and its reason is phrased in those terms. Widening it to a
   second, unrelated kind is a change to that surface and not to the recognizer,
   so it is deferred rather than folded in here.
2. **Does `>yesterday` deserve to work?** It does today in prose, and this
   change inherits it by reusing `day_word`. A sigil naming a past day is odd
   but it is also unambiguous, and refusing it would mean the sigil and the
   prose reading the same word differently — the exact drift this change is
   trying to avoid. Left working, noted here because it was a choice.
