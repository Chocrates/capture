## 1. Finding the references

- [x] 1.1 `Log::blobs_of(tgt)` — the blob hashes carried by the events a purge is about to destroy, read while the bodies still exist. Same event selection as `destroy_bodies`, so the two cannot disagree about which events a purge is about.
- [x] 1.2 `Log::blob_referenced_elsewhere(hash, tgt)` — whether any other capture with an intact body names the same hash. A `NULL` body is a note already redacted and is not a reference worth keeping bytes for.
- [x] 1.3 A capture this build cannot decode counts as a reference. Keeping bytes nobody wanted is recoverable; destroying bytes somebody references is not.
- [x] 1.4 **Captures only, and the first draft got this wrong.** The scan looked at every event with a body, and a redaction *retains its own payload* — which is not a capture, fails to decode as one, and so hit the "unreadable counts as a reference" rule. The result was that the second of two notes sharing a recording kept its bytes for ever. Caught by the shared-blob test; fixed by filtering on `kind = CAPTURE`, which is the only kind that carries a blob reference anyway.

## 2. Destroying them

- [x] 2.1 A `BlobsDestroyed` phase between `ProjectionPurged` and `BodyDestroyed`.
- [x] 2.2 Before `destroy_bodies`, because the hash lives in the body. It is also the only order the module's crash rule permits: a crash here leaves a body that still names a blob and a next run that can finish, where the reverse leaves bytes nothing can ever name.
- [x] 2.3 Destroying a blob that is already gone is not an error — `destroy` returns `Ok` for a missing file, which is what makes the resume path work.
- [x] 2.4 The path threaded through `execute` and `resume` and their five call sites.

## 3. The confirmation

- [x] 3.1 The prompt names the recording. §1.1's one prompt can only carry its weight if it says what actually goes.

## 4. The scan that should have caught this

- [x] 4.1 `tools/unwired.py` stripped comments and not string literals, so `"This will permanently destroy the content of:"` made `blobs::destroy` look called. It strips strings now.
- [x] 4.2 That surfaced two more. `fold::reaches` is allow-listed and says so in its own doc — the fold answers the question from branch tips and this is kept as a checked cross-statement of the equivalence. `ReviewState::declined` was **the same duplication as `interval_ms`**: `deferred >= 2` in Rust and `COALESCE(r.deferred, 0) < 2` in the SQL, two copies of one rule. Now one `DECLINED_AFTER` constant, used by both.

## 5. Seeing it

- [x] 5.1 A note with a recording, redacted: the bytes are gone from disk.
- [x] 5.2 Two notes sharing one recording — identical bytes are one blob, by design — and redacting the first keeps it while redacting the second destroys it.
- [x] 5.3 Re-running the whole purge finds the bytes already gone and does not error, which is the resume path.
- [x] 5.4 A note with no recording redacts exactly as before, and its capture body is gone.
- [x] 5.5 Verified non-vacuous: stubbing out the destroy call fails three of the four.

## 6. Not fixed here, and said plainly

- [ ] 6.1 Blobs orphaned by a redaction performed **before** this change stay orphaned. Their hashes went with the bodies and nothing can name them now. A `capture blobs --orphaned` would find them — any blob on disk whose hash appears in no intact capture — and that is a diagnostic rather than part of this.
