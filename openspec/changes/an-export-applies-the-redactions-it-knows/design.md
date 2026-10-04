## Context

`export` strips bodies for credential-flagged notes only. Redacted content stays
out of exports incidentally — once destruction has run, `destroy_bodies` has
already emptied the body — so the gap is exactly the interval between ordering
and carrying out.

## Goals / Non-Goals

**Goals.** An export written after a redaction does not carry its content. The
two reasons for withholding are told apart.

**Non-Goals.** Reaching existing export files. Applying redactions that have not
arrived. Changing the window.

## Decisions

### The queue, not the log

`redact_queue` already carries what is wanted — target and cancelled — and the
fold maintains it. Re-deriving the set by decoding every `RedactBody` in the log
would be a second implementation of what the fold already computes, including
the cancellation rule, which is the kind of duplication this project has been
paying for all week.

Notes whose destruction has already run need no entry: their bodies are `None`
and there is nothing to strip.

### Emptied, not omitted

The same rule the credential strip follows, for the same reason its comment
gives: dropping the event would leave a hole in its origin's numbering, and a
hole is the one state this design treats as unrecoverable. An export is a
restore source, so it has to stay dense.

### Reported apart

A credential flag is a guess that over-flags on purpose and may be cleared with
`capture unflag`. A redaction is a decision. Printing one count for both would
put "if one of those is an ordinary note, unflag it" next to notes where that
advice is wrong.

## Risks / Trade-offs

- **A restore from a new export will not carry redacted notes' text.** → That is
  the intent, and the redaction travels in the export too, so a restore applies
  it again through the gate.
- **A redaction cancelled after an export was written leaves that export
  missing text.** → The export is a point-in-time copy and the note is intact
  locally. Losing text from one backup file is the safe direction.

## Migration Plan

None. Export files already written are unchanged, and §1.1 says plainly they
cannot be reached.

## Open Questions

- **Should an export refuse to run while a destruction is pending?** It would
  close the window rather than narrow it, at the cost of a backup not happening
  — which is the wrong trade for a system whose cardinal sin is data loss.
