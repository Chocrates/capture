## Context

`recognize` has two tiers. Tier 1 reads sigils and returns immediately; tier 2
infers from words. The early return is what makes the sigil win, and it is also
what loses the verb: `head_is_verb` is computed *after* it, so a sigil date
never reaches the composition that turns a day into a due date.

Tier 2's date anchors come from `Class::Weekday`, `Class::DayWord` and
`Class::Month` followed by a number. An ISO date is none of those, so it is not
a date anywhere except behind `>`. A number followed by a month is not one
either.

## Goals / Non-Goals

**Goals.** A verb and a sigil date read as a task due. An ISO date read as a
date. A month name read either way round.

**Non-goals.** Numeric dates. How a bare month name picks its year. Times.

## Decisions

### The sigil path composes rather than returning

`head_is_verb` moves above the sigil check, and the sigil branch asks the same
question the inferred branch asks: is the head a verb, and is this a day rather
than a moment.

This does not weaken `recognizer/spec.md:6`. That rule is about *which date*
wins when two readings disagree, and nothing here makes an inference compete
with a sigil. The sigil keeps naming the date, at full confidence. What changes
is that the verb is also read, and it was always answering a different question.

*Rejected:* a separate sigil for "due", so `pay the invoice !due 2026-09-20`.
It makes somebody learn a second marker to say something the sentence already
says, and §3.1.1 keeps the sigil set small on purpose.

### An ISO date is its own token class

`Class::IsoDate(FloatingDate)`, decided in the tokenizer where every other shape
is decided, rather than by the anchor code re-parsing text.

The tokenizer already owns "what is this run of characters", and a date is
exactly that question. Putting it anywhere else means two places that know what
a date looks like.

Validity is checked when the class is assigned, so `2026-13-45` is an ordinary
word rather than a date that fails later.

### Day-before-month is a second arm, not a reordering

`Class::Number` followed by `Class::Month` becomes a complete date, beside the
existing month-then-number arm. Both produce `Anchor::CompleteDate`, so
everything downstream — the verb composition, the confidence band — is
unchanged.

The number is checked as a day of the month first, which is what keeps `in 3
weeks` out: `3` is followed by a unit, not a month name.

### Numeric dates stay unread, and the spec says so out loud

`20/09` and `09/20` are the same characters. Nothing in a capture says which
convention its author uses, and a locale setting would be a guess about a
*person* rather than about the text.

Worth stating as a requirement rather than leaving as an absence, because the
obvious next change is to add them, and the reason not to is not obvious.

## Risks / Trade-offs

- **A sentence that opens with a task verb and names a date for something else.**
  `call sarah about the meeting on friday` becomes a task due Friday rather than
  a note about a Friday meeting. → Already true for every inferred date; this
  change makes sigils behave the same way rather than introducing it. The
  existing band is 80, below certainty, and a wrong kind is correctable from any
  surface now.
- **An ISO date inside something that is not a date.** A version string, an
  identifier. → Must be a whole token and a valid calendar date, so `v2026-09-20`
  and `2026-13-45` are words.
- **`20 sep` colliding with a quantity.** → A month name has to follow, and no
  unit is a month name.

## Migration Plan

None. Recognition is derived at read time. Existing captures are re-read by the
sweep on the next grammar bump, which is what `GRAMMAR_VERSION` is for — and a
capture whose kind changes because of it is one that was being read wrongly.

## Open Questions

- **Should `GRAMMAR_VERSION` bump?** Yes, and that is the point: it re-reads
  every capture that was parsed by the old rules, which includes every one that
  landed on the wrong half of §3.6's table. A pinned correction still wins.
- **Does a bare ISO date want the same 100% a sigil gets?** Taken as a complete
  date at the existing band for one, rather than inventing a third level of
  certainty — it is exactly as unambiguous as `sep 20` and should not read as
  more so.
