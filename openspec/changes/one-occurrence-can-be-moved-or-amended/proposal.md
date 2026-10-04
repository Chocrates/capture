# one-occurrence-can-be-moved-or-amended

## Why

DESIGN §3.5 lists four exception events, and its prose states the capability
outright: "You can complete, skip, or **move** next Tuesday's gym session
without anything named 'next Tuesday's gym session' existing in any table." Two
of the four cannot be authored from anywhere.

| kind | meaning (§3.5) | writer |
|---|---|---|
| `occ_done` | completion | `mutate`, CLI, FFI, window, phone |
| `occ_cancel` | this occurrence skipped | `mutate::skip_occurrence` |
| `occ_move` | **this occurrence only: new start/end** | only `restore_occurrence`, hardcoding `start_ms: occ` |
| `occ_edit` | **this occurrence only: different title, location, notes** | **nothing** |

`restore_occurrence` is the sole non-test construction of an `OccMoveBody`, and
it is deliberately degenerate: it moves an occurrence back to its own original
start, because the fold has no un-cancel event and a move beats a cancel. That
is well argued and correct. It also means the only move anybody can make is the
one that changes no time at all.

Everything underneath is built and tested. The fold decodes `OccMoveBody` and
`OccEditBody`, resolves move-beats-cancel, and marks a DST transition as `gap`
or `ambiguous`; the projection stores `start_ms`, `end_ms`, `shift`, `title`,
`location` and `notes`; the desktop renders the shift marker. All of it serves a
gesture nobody can make — which is this project's own definition of not shipped.

The skip half of §3.5 was wired by `one-occurrence-can-be-skipped`, and that
change found a real bug while doing it: `OccState::visible()` was read by
nothing but its own test, so "a move beats a cancel" was true in the fold and
absent from the column every surface filters on. This is the remaining half of
the same mechanism, and it is the half the DST handling is attached to.

## What Changes

- `mutate::move_occurrence(paths, name, occ, start_ms, end_ms)` writes an
  `OCC_MOVE` naming a new time, beside `restore_occurrence`, which keeps its
  degenerate form and its reason.
- `mutate::amend_occurrence(paths, name, occ, title, location, notes)` writes an
  `OCC_EDIT`. The first writer of that kind anywhere.
- Both refuse a note with no occurrences, the way `skip_occurrence` does —
  naming what produces them rather than writing an event that changes nothing
  when folded.
- `capture move <series> <occ> <start-ms> [<end-ms>]` and
  `capture amend <series> <occ> [--title T] [--location L] [--notes N]`,
  naming an occurrence exactly as `capture done` and `capture skip` do.
- Tests for what the fold already implements and nothing could reach: a moved
  occurrence showing at its new time with the rest of the series untouched, a
  completion written before a move surviving it, and an amended occurrence
  carrying its own title while its siblings carry the series'.

## Capabilities

### Modified Capabilities

- `recurrence` — moving and amending a single occurrence are reachable.

## Non-goals

- **No "this and future".** §3.5's third scope is a split, which exists and is
  wired (`capture split`). This change is only the first scope, "this
  occurrence".
- **No window or phone surface yet.** The store function is the thing that makes
  two surfaces agree, and §8.4 wants the window to have it — but the window is
  already behind the phone on four other capabilities, and that is one change
  about the window rather than a tail on this one.
- **No new DST behaviour, and the premise there was wrong.** I expected the
  fold to mark a move onto a gap or an ambiguous hour, and it does not: `shift`
  is filled from the *expansion's* value — the shift of the slot the rule
  produced — and the fallback path for exceptions the rule no longer reaches
  writes `NULL`. Neither recomputes it for where the occurrence was moved to.

  Left alone, with a test stating today's answer. §4.4's reasoning on that
  column is "a time that moves on its own with no explanation is what makes
  someone stop trusting the whole system", and a move is not the system moving
  something on its own. Whether a calendar should still say "you have put this
  at a wall time that happens twice" is a real question and a different change.
- **No un-cancel event.** `restore_occurrence` stays a move to the original
  start, for the reason its doc comment gives: adding an un-cancel would make
  the move-beats-cancel conflict rule ambiguous rather than simpler.
