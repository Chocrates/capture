# Design

## What already exists, and what is missing

Nothing here needs new mechanism. The missing part is two functions and two
commands.

| layer | move | amend |
|---|---|---|
| body type | `OccMoveBody { occ, start_ms, end_ms }` | `OccEditBody { occ, title, location, notes }` |
| fold arm | decodes it, resolves move-beats-cancel | `apply_occ_edit` |
| projection | `start_ms`, `end_ms`, `shift` | `title`, `location`, `notes` |
| surface rendering | the desktop draws the shift marker | rows carry the columns |
| **writer** | **only `restore_occurrence`, hardcoding the original start** | **none** |

So this change is almost entirely tests — which is the honest shape of wiring up
something whose machinery was built and never reached, and it is where the value
is. `one-occurrence-can-be-skipped` did exactly this and the act of *using* the
mechanism exposed `OccState::visible()` being read by nothing, with the
move-beats-cancel rule absent from the column every surface filters on. The fold
was correct throughout; no amount of reading it would have shown the bug.

## `restore_occurrence` keeps its shape

The temptation is to express restore as `move_occurrence(.., occ, occ, None)`
now that a general move exists. Not doing that. `restore_occurrence`'s doc
comment is the argument:

> there is no un-cancel event, and adding one would make that conflict rule
> ambiguous rather than simpler. Moving something to where it already was is a
> no-op about time and a statement about cancellation.

Restoring and moving are different intents that happen to share an event kind.
Collapsing them would make the call site read as "move this to where it is",
which is exactly the sort of clever indirection that is obvious today and
unreadable in a year. Both stay, both write `OCC_MOVE`, and the duplication is
two lines of body construction rather than a decision made twice.

## Naming an occurrence

`(series, original_start)` — the iCalendar `RECURRENCE-ID` approach §3.5 chose,
because it names an occurrence that has never been materialised anywhere. Both
commands take the series and the original start, exactly as `capture done` and
`capture skip` do, and the original start does not change when the occurrence
moves. That is what lets a completion written before a move still apply after
it, and it is worth a test rather than an assumption.

## Refusing a note with no occurrences

`skip_occurrence` already refuses through `series()`, which exists for this and
whose comment says why: an exception naming a plain note "would sit in the log
for ever recording a mistake that changes nothing when folded — the system
silently right and the person silently wrong". Both new functions go through the
same helper, so there is one refusal rather than three.

## The DST case is the reason this half matters

A move is the only way to put an occurrence on a time that may not exist. The
fold already marks the result `gap` or `ambiguous`, §4.4's reasoning is recorded
on the column — "a time that moves on its own with no explanation is what makes
someone stop trusting the whole system" — and the desktop renders it.

Until now nothing could produce that state, so the marking, the rendering and
the reasoning were all untested against a real move. This change makes it
reachable, which means it can finally be asserted rather than believed.

## The CLI surface

`capture move <series> <occ> <start-ms> [<end-ms>]` and
`capture amend <series> <occ> [--title T] [--location L] [--notes N]`.

Milliseconds rather than a parsed date, matching `skip` and `done`, which take
the occurrence that way already. A friendlier spelling for naming occurrences is
a worthwhile tidy across all four commands at once, not a thing to introduce
here for two of them and leave inconsistent.

`amend` rather than `edit`: `capture edit` rewrites a note's text, and a reader
who confuses them would be rewriting the series instead of one occurrence —
which is precisely the scope confusion §3.5 says implementations get wrong.
