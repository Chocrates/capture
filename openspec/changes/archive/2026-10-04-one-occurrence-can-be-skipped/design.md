## Context

Five exception kinds were numbered together in m2 (`occ_move` 19, `occ_edit` 20,
`occ_cancel` 21, `occ_done` 22, `occ_undone` 23) and two were wired to a
surface. The other three have bodies, fold arms, a projection column, a conflict
rule, calendar rendering and tests, and nothing emits them.

So this change writes no encoding, no fold arm and no projection. It is surface
over a mechanism that is already finished and already tested — which is why it
is small, and why it was worth finding.

## Goals / Non-Goals

**Goals.** Skip one occurrence. Restore it. One naming convention with the
commands that already exist.

**Non-Goals.** Moving to a new time, `occ_edit`, the phone, the window, any new
event kind.

## Decisions

### Restoring is a move, not an un-cancel

`apply_occ` sets `cancelled = true` and nothing sets it false. That is not an
omission: m2's task 3.4 implemented `occ_move` beating `occ_cancel` on the same
occurrence precisely so a cancellation can be undone *without* an event whose
only job is to undo one.

The reasoning it records is worth keeping in view — a resurrected occurrence is
visible and one gesture away, where a wrongly cancelled one is invisible and
silently missed. The asymmetry favours the recoverable mistake.

So `unskip` writes `occ_move` with `start_ms` set to the occurrence's own `occ`,
which *is* its original start by definition. Moving something to where it
already was is a no-op about time and a statement about cancellation.

*Rejected:* an `occ_uncancel` kind. A sixth number, frozen for ever, to express
what the fold already resolves — and it would make the cancel/move conflict rule
ambiguous rather than simpler.

### The occurrence is named in milliseconds, like its siblings

`capture done <series> <occ>` takes the original start as an integer. That is
awkward for a person and deliberate for a harness (§8.6), and this change keeps
it rather than inventing a second convention for two of five commands.

Whether a person should be typing instants at all is a real question. It is the
same question for `done`, `move` and `edit`, and it should be answered for all
five at once by whoever designs the surface that shows occurrences — not here,
halfway, for the two that happen to be getting written today.

### Refusing a note that is not a series

A `skip` against a plain note is a mistake worth naming. The check is the same
`find` every other mutation uses, plus the series' occurrences: a note with none
is not a series, whatever it says.

*Rejected:* writing the event anyway and letting the fold ignore it. An event in
the log for ever, recording a mistake, that changes nothing when folded — the
system would be silently right and the person silently wrong.

## Risks / Trade-offs

- **`unskip` on an occurrence that was never cancelled writes a move to where it
  already is.** → An event that changes nothing, which is the honest encoding of
  what was asked. The alternative is a refusal that depends on current state,
  and §6 prefers the idempotent gesture.
- **Milliseconds are a poor way to name a Thursday.** → Named above, deferred
  above, and shared with the two commands that already do it.

## Migration Plan

Nothing. No encoding, no projection, no schema.

## Open Questions

- **How should a person name an occurrence?** The real question behind the
  decision above, and it belongs with the surface that lists them.
- **Should `capture agenda` show cancelled occurrences?** It is a view decision
  and `occurrences_of` already flags them rather than filtering, so the data is
  there either way.
