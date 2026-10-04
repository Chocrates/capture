## Why

Redacting a voice note destroys the event body and leaves the audio on disk.

DESIGN §1.1 states the behaviour as implemented:

> Redaction therefore runs a defined sequence — overwrite, checkpoint, `VACUUM`
> — and **blob redaction overwrites the file before unlinking**.

`blobs::destroy` does exactly that: it opens the file, overwrites it, then
unlinks. It is tested. And `purge.rs` never calls it — `grep -c blob
crates/capture-store/src/purge.rs` returns **0**. Nothing anywhere in
`crates/*/src` or `apps/*/src` calls it.

So the one irreversible operation in the system, whose entire purpose is
destruction, destroys the text and leaves the recording. A person who redacts a
voice note has been told the content is gone. On the phone — where voice capture
happens, and §8.4 calls the phone the constrained device — the bytes stay.

This is the operation §1.1 says must not be trained away by appearing routinely,
and §6 rates as unrecoverable when it goes wrong. It going *right* is supposed
to be the thing somebody can rely on.

### Why it was never noticed

`tools/unwired.py` should have caught it and could not: `apps/capture-cli/src/
write.rs:604` contains the string literal `"This will permanently destroy the
content of:"`. The scan strips comments but not string literals, so the word
`destroy` in a confirmation prompt made `blobs::destroy` look called. That is a
live instance of the blind spot the tool documents under `cosign`.

## What Changes

- **Redaction destroys the blob bytes** of the events it is destroying, through
  the function written to do it.
- **Only when nothing else references them.** §5.6: *"Dropping the last
  reference to a blob overwrites and unlinks it."* The same recording captured
  twice is one blob with two references, and destroying it because one of them
  was redacted would destroy somebody else's note. A hash still referenced by a
  capture whose body is intact is kept.
- **Before the body, not after.** The blob hash lives *in* the capture body, so
  once `destroy_bodies` has run there is nothing left to say which blob the note
  referenced. Destroying blobs first also keeps the module's crash rule: a crash
  between the two leaves a body that still names a blob the next run can finish
  destroying, where the other order leaves a blob nothing can ever name again.
- **A phase of its own**, so an interrupted purge resumes into it like every
  other step.
- **The scan stops being fooled by a string literal**, so the next one of these
  is caught rather than reasoned out.

## Capabilities

### Modified Capabilities

- `event-log`: that redaction destroys content held outside the log, and when
  it must not.
- `cli`: that the confirmation says what will be destroyed, including a
  recording.

## Impact

- `crates/capture-store/src/purge.rs` — the new step and its phase.
- `crates/capture-store/src/log.rs` — the blob references an event carries, read
  before the body goes.
- `tools/unwired.py` — strip string literals as well as comments.

## Non-goals

- **Eviction.** `blobs::evict` is also uncalled and is a different operation on
  purpose — §5.6 keeps them distinct precisely so "freed some space" never
  becomes "lost the recording". It is recorded in `tools/unwired-allow.txt`
  with its own reason and needs its own change.
- **A `blob_refs` table.** DESIGN §908 describes one and the projection does not
  have it. Scanning the log for other references is adequate for an operation
  that already runs `VACUUM`, and inventing a projection table for it would be a
  second place for the answer to live.
- **Making the overwrite a guarantee.** §1.1 is explicit that on flash it cannot
  be one. This change makes the system do what it says; it does not change what
  it says.
