## Why

DESIGN §1.1:

> **`redact_cancel` is as sticky and as eagerly delivered as `redact`.** … and
> **a cancel arriving after its target has been destroyed is recorded and
> reported rather than silently discarded**.

Recorded, yes. Reported, no.

A cancellation that arrives from a peer after this device has already carried
the destruction out is folded like any other: it joins `cancelled`, the
governing redaction is marked cancelled, and then — because the content is
already gone — the fold takes the `text.is_none()` branch, sets `redacted_at`,
and drops the fact on the floor. Nothing anywhere says a cancellation came too
late, and no surface could say it, because the state is not kept.

The situation is ordinary, not exotic. Two devices, one redaction: you change
your mind and cancel on the phone, the desktop has already destroyed. The phone
says *"Cancelled. Nothing was destroyed."* The desktop silently keeps the
content gone. Both devices report success, neither mentions the other, and the
only way to find out which happened is to go looking for the note.

§6 rates a wrongly-destroyed note as unrecoverable, and this is the one case
where somebody actively tried to prevent it and was told they had.

The local half of this was closed by removing the stale queue row that let
`cancel_redaction` claim a recovery it had not performed. This is the other
half: a cancel that arrives from somewhere else.

## What Changes

- **The fold records that a cancellation arrived too late**, on the note it was
  about — the one place that knows both that the content is gone and that the
  governing order was withdrawn.
- **It is reported as wanting attention**, naming the note. A person who
  cancelled a redaction believes the content survived; being wrong about that
  silently is the failure the design is guarding against.
- **It is not treated as an error, and nothing is retried.** The content is
  gone. What is owed is the truth, not a recovery that cannot happen.

## Capabilities

### Modified Capabilities

- `projection`: that a cancellation arriving after destruction is kept.
- `cli`: that it is reported.

## Impact

- `crates/capture-core/src/fold.rs` — a field on the note, set in the branch
  that currently discards this.
- `crates/capture-store/src/projection.rs` — a table for it, and
  `PROJECTION_SCHEMA` 11 → 12.
- `apps/capture-cli/src/ops.rs` — the report.

## Non-goals

- **Recovering the content.** It is destroyed. The pre-redaction snapshot is a
  separate mechanism with its own window and its own command; where one exists,
  `capture recover` already offers it.
- **Stopping the destruction happening.** A cancel that arrives after the fact
  arrives after the fact. §1.1's window is what narrows this, and it is a
  property of the clocks involved rather than something a replica can fix.
- **Reporting it on the phone or in the window.** The condition belongs
  everywhere eventually; the command line is where the rest of redaction's
  reporting already is.
