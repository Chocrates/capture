## 1. A history that exists

- [x] 1.1 `Log::redactions_by(dev)` — an origin's redactions with their target counts, decoded from the bodies the log retains. The log is the only place it can come from: `destroy_bodies` excludes `REDACT` so a redaction goes on destroying matching events arriving later, which is what leaves the targets countable after the notes are gone.
- [x] 1.2 `Store::redaction_history` deleted. It selected from a table redactions never reach, was not per origin, counted events rather than targets, and had no test.
- [x] 1.3 A body this build cannot decode counts as one target rather than none — an unreadable body must not be the way past the bound.

## 2. Asking the right question

- [x] 2.1 The drain reads the redaction's origin, timestamp and target count from the log.
- [x] 2.2 The window is measured around the redaction's own time, not the drain clock. A runaway issues in seconds; measured from "now", its output arriving after a month offline scores nothing and all proceeds — the receiving-replica case §1.1 says the bound is for. It also makes two replicas draining the same batch at different moments reach the same verdict.
- [x] 2.3 **Only redactions *earlier* than this one count, and the first version got this wrong.** Counting every redaction in the window makes the check order-independent, so 266 of them each saw the other 265 and *all* were held — no first 256, and a bulk clear-out one over the line stopped completely rather than stopping at the line. §1.1 holds "further" redactions, which needs an order. `(at, seq)` is a total order within an origin, so every replica holds the same ones. Caught by the test asserting exact counts rather than "at most".
- [x] 2.4 A release recorded in `ops` skips the ceiling for that redaction.

## 3. Held is visible

- [x] 3.1 `capture status` reports held destructions as wanting attention, with how many and which ids are waiting.
- [x] 3.2 `capture redact --release <id>`, beside `--cancel` rather than a command of its own: both are things done to a redaction by the id it printed.
- [x] 3.3 Releases are local state, never events. A release says only that this device is satisfied, and a compromised device able to emit them would release its own runaway.

## 4. Seeing it

- [x] 4.1 An origin over the ceiling has **exactly** the ceiling carried out and the remainder held. Asserted exactly, because "at most" is also satisfied by holding everything — which is what the first implementation did.
- [x] 4.2 What is held still has its **bytes**. Asserted against the log rather than readability: a redaction hides its target from every view on ingest immediately and only the destruction is delayed, so a held note is already invisible and its body is still there. The first version of this test asserted readability and failed for the wrong reason.
- [x] 4.3 Ordinary bulk use — tens of notes — is never held.
- [x] 4.4 A clustered batch drained a year later is still held.
- [x] 4.5 Releasing carries out what was held, asserted by the bytes.
- [x] 4.6 Verified non-vacuous: feeding `check` an empty history, which is what the old code did, fails the exact-count test.

## 5. Not done, found while here

- [ ] 5.1 A destroyed note keeps its `redact_queue` row — `purge_note` clears text, FTS, links, mentions and list items but not the queue — so every later drain re-runs the purge idempotently and counts it again. Harmless but wasteful, and it makes `Drained.destroyed` a poor thing to assert on. Its own small change.
- [ ] 5.2 More than 64 targets is still truncated by the fold rather than refused. Separate from the ceiling and listed in the proposal; it is latent while every shipping `RedactBody` names exactly one target.
