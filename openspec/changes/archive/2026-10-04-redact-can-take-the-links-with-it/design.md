## Context

`Session::redact` names one target. Multi-target redactions were made to work
correctly in a recent change — the queue is keyed by the pair, the fold refuses
over the cap, and the incremental path sees every target — and that change
deliberately built no surface for them. This is that surface, and the first
real consumer.

`links_to(tgt)` already answers "which notes point here", with their slugs.

## Goals / Non-Goals

**Goals.** Redact a note and its backlinks in one redaction. Name every note in
the confirmation. Refuse rather than truncate.

**Non-Goals.** Editing text. Doing it by default. Transitive links.

## Decisions

### Destroy the linking notes, not their text

§1.1 says the command *"redacts inbound linking text alongside the target"*,
and §3.1.3 keeps captured text verbatim for ever. Those only reconcile one way:
the linking notes are redacted, not rewritten.

Rewriting would be worse on its own terms. A note is somebody's record of what
they wrote; editing it to remove a word that appears there because of a
*different* note's name is the system deciding what somebody meant.

### One hop

A note linking to a note linking to the target has copied nothing of the
target — it copied the middle note's name. Following further would mean a
single flag could walk an arbitrary distance through the archive, which is how
a deliberate act becomes an accident.

### Refuse over the cap

§1.1 caps a redaction at 64 targets and the fold now holds anything over it as
malformed. So redacting 70 linked notes cannot be expressed as one redaction,
and splitting it into two would be this command inventing a policy about which
notes go first. It refuses and says how many there were.

*Rejected:* redacting the target alone and warning about the rest. The whole
point of the flag is that the copies go together; half of it is the hole
§1.1 already describes.

### One redaction, not several

The targets travel in one `RedactBody`, which means one cancellation covers all
of them. Several separate redactions would make cancelling a mistake a matter
of catching each one inside its own window.

## Risks / Trade-offs

- **A valuable note can be destroyed for linking to a sensitive one.** → That
  is the trade the flag exists to let the user make, which is why it is a flag
  and why the confirmation lists every name.
- **The blast radius of a familiar confirmation grows.** → Only with the flag,
  and the prompt says what is going. §1.1's concern is a prompt that appears
  routinely; this one appears only when asked for.

## Migration Plan

None. No new event kind, no schema.

## Open Questions

- **Should the window reflect how much is going?** A sixty-four note redaction
  is a bigger mistake than a one-note one and gets the same thirty minutes.
  Arguably the ceiling already covers runaway volume, and this is deliberate
  volume; worth revisiting if it ever bites.
