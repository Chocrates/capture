## 1. Reading the log

- [x] 1.1 `Log::aside_rows()` — id, origin, sequence, reason and authored instant for every event the log has set aside, through the column `Log::is_aside` was written for and nothing read.
- [x] 1.2 The origin is parsed back into a `Dev` rather than carried as the hex the column stores, so a quarantine row is written the same shape by every writer and there is one conversion rather than two spellings of an origin in flight.

## 2. Rebuilding with them

- [x] 2.1 `projection::rebuild` takes the rows and writes them, so a projection is complete whoever built it. Inside rather than beside, because "the caller remembers" is the arrangement that caused this.
- [x] 2.2 Eleven call sites. The four production ones read from the log; the synthetic tests pass `&[]`, which is honest — a test that builds a list of envelopes by hand has no log and nothing set aside.

## 3. Seeing it

- [x] 3.1 Set aside at ingest, rebuilt, still reported — with its reason intact.
- [x] 3.2 **The remedy.** Set aside by a retirement, projection rebuilt *between* the retirement and its reissue — which is the ordinary case, since a schema bump rebuilds every archive — then reissued with a later head, and the event below it is re-admitted. The log's own mark is checked as well as the report, because releasing the row without clearing the mark would look like success and change nothing.
- [x] 3.3 Verified non-vacuous: stubbing out the restore fails both.
- [x] 3.4 Both needed `reconsider()` before the offer, as the existing re-admission test does — without it the origin is not yet settled and the event is refused rather than set aside.

## 4. Not done

- [x] 4.1 `first_seen` for a restored row is the event's authored instant, not when this device saw it. That moment existed only in the projection being replaced. Written into the spec rather than left for somebody to discover.
- [ ] 4.2 Whether `capture status` should distinguish "set aside" from "held" — different tables answering different questions, and `capture held` now exists for the second. A surface question, not this change.
