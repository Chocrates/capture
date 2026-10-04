## Why

DESIGN §1.1, limit 3 — the section on what redaction cannot reach:

> **Exports are outside the reach of redaction.** … **The exporter therefore
> applies retained redactions when writing *new* export files**, and the UI
> states plainly that existing exports and any offsite copies must be pruned by
> hand.

It does not. `export` builds its strip set from one source:

```rust
let flagged: Vec<_> = state.credential_flagged()?;
let strip: BTreeSet<_> = flagged.iter().map(|(id, _)| *id).collect();
```

Nothing consults `redact_queue`, `notes.redacted_at`, or any redaction at all.
Exports are clean of redacted content only incidentally — by the time the
destruction has been carried out, `destroy_bodies` has already emptied the body,
so there is nothing left to write.

The gap is the time between ordering a destruction and carrying it out.
Redaction hides the note from every view immediately and destroys the body
`REDACT_WINDOW_MS` later. An export firing in that window writes the full
plaintext of a note already ordered destroyed into a **brand-new file** — one
created after the redaction was issued, which redaction can then never reach.

That window is not only thirty minutes. §8.7 makes a startup the floor for
deferred work, so on a machine with no overlay running, destruction waits for
the next command that writes — which can be days. `install.sh --with-export`
sets up a daily export and §8.7 recommends wiring it to a timer, so the two are
arranged to meet.

Partly mitigated and worth saying: `note_stale_exports` raises a tier-one
"export pruning required" item after a purge, so the leak is surfaced. But it is
surfaced as "go and delete files by hand", for a file the design says should
never have carried the content.

Found by auditing DESIGN's present-tense claims against the code.

## What Changes

- **An export strips the body of any note a redaction names**, whether or not
  the destruction has been carried out yet.
- **Reported separately from the credential strip**, because the reasons differ
  and so does what a person should do about it. A credential-flagged note may
  be a false positive to clear with `capture unflag`; a redacted one is
  content somebody has ordered destroyed and there is nothing to undo.
- **Cancelled redactions do not strip.** A cancellation means the note stands.

## Capabilities

### Modified Capabilities

- `cli`: that an export applies the redactions the device knows about.

## Impact

- `apps/capture-cli/src/backup.rs` — the strip set and the report.

## Non-goals

- **Reaching existing export files.** §1.1 is explicit that it cannot, and the
  stale-export warning stays exactly as it is. This change is about the files
  written *after* a redaction, which is the half the design claims.
- **Stripping events a redaction will name later.** A redaction that has not
  arrived cannot be applied; that is what retaining it is for, and the restore
  path offers it to the gate again.
- **Changing the window.** Shortening it would narrow the gap and not close it.
