## Context

`purge.rs` runs a five-step sequence chosen so a crash at any point leaves a
state the next run can finish. Step 3 is `log.destroy_bodies(tgt)`, which sets
`body = NULL` for the capture and everything targeting it.

The blob hash lives in `CaptureBody.blob`. The fold does not track it — there is
no `blob_refs` table, despite DESIGN §908 describing one — so the body is the
only place the hash exists.

`blobs::destroy` overwrites then unlinks, is tested, and has no caller.

## Goals / Non-Goals

**Goals.** Redaction destroys the recording. A shared blob survives. An
interrupted purge still finishes.

**Non-Goals.** Eviction. A `blob_refs` table. Making the overwrite a guarantee
on flash, which §1.1 already says it is not.

## Decisions

### Blobs go before bodies

The hash is *in* the body. Destroying the body first leaves nothing that can
name the blob, so the bytes would survive with no record that they should not —
the silent half of a half-finished purge, which is the thing the phase marker
exists to prevent.

Taking them in the other order is also what the module's crash rule asks for:

- Crash after blobs, before bodies → a body that still names a blob. The next
  run re-reads it, destroys a blob that is already gone (a no-op; `destroy`
  returns `Ok` when the file is missing) and carries on.
- Crash after bodies, before blobs → a blob nothing can name. Unrecoverable by
  any later run.

One of those is finishable and the other is not, which decides the order.

### A blob is destroyed only when the last reference goes

§5.6: *"Dropping the last reference to a blob overwrites and unlinks it."* The
same recording captured twice is one blob with two references, by design — the
hash is the address, so duplicates are impossible.

So before destroying, the log is scanned for any *other* capture that references
the hash and still has its body. A body that is `NULL` is a note already
redacted and is not a reference worth keeping bytes for.

This is a full scan of capture bodies. It is acceptable here and nowhere else:
redaction already runs `VACUUM` over the whole database, it is rare, it is
deliberate, and it is the one operation where being slow and right beats being
fast.

*Rejected:* a `blob_refs` table in the projection. It would be a second place
for the answer to live, derived from the same bodies, and wrong in exactly the
case that matters — a projection rebuilt while a purge is half-finished.

*Rejected:* destroying unconditionally. It would make redacting one note destroy
another note's content, which is the cardinal failure committed by the operation
least able to undo it.

### The confirmation says so

§1.1 makes this the one prompt that must not be trained away, and a prompt can
only carry that weight if it names what actually goes. "This will permanently
destroy the content of: <note>" does not tell somebody the audio is included.

## Risks / Trade-offs

- **A full scan of capture bodies per redaction.** → Bounded by the log, beside
  a `VACUUM` that is heavier, on an operation that is rare and deliberate.
- **A blob referenced by an event this build cannot decode.** → Treated as a
  reference and kept. Keeping bytes nobody wanted is recoverable; destroying
  bytes somebody still references is not.
- **The overwrite is still best-effort on flash.** → §1.1 says so already and
  this does not change it.

## Migration Plan

Nothing to migrate. Blobs already orphaned by a redaction performed before this
change stay orphaned — their hashes went with the bodies, and nothing can name
them now. That is worth saying plainly rather than implying the fix is
retroactive.

## Open Questions

- **Should an already-orphaned blob be findable?** Any blob on disk whose hash
  appears in no intact capture body is one of these. A `capture blobs --orphaned`
  would find them, and it is a diagnostic rather than part of this change.
