## 1. The queue

- [x] 1.1 `PRIMARY KEY (redact_evt, tgt)` — the pair is what a queued destruction is. It was keyed by the redaction alone, and the fold writes a row per pending note, so every target overwrote the last.
- [x] 1.2 `PROJECTION_SCHEMA` 10 → 11 with its changelog entry. A key change is not something `CREATE TABLE IF NOT EXISTS` applies to a table that already exists, so the rebuild into a fresh file is what carries it.

## 2. The fold

- [x] 2.1 More than the cap is held as malformed and applies to nothing. It used to `take` the first 64 and drop the rest with no record, so a redaction naming a hundred reported success and left thirty-six intact — and a redaction of the first 64 is not what anybody asked for.
- [x] 2.2 An undecodable body is held rather than skipped. It was `continue`, which made an unreadable destruction order indistinguishable from no order at all.

## 3. The incremental path, which the key fix alone did not reach

- [x] 3.1 **Found while testing.** `affected()` mapped each event to its header target, and a redaction's header holds one where its body may name sixty-four. So on the cheap path the other targets were never re-written and their destructions never queued — the header's note destroyed, the rest intact until something happened to rebuild.
- [x] 3.2 `affected` now extends with a redaction's body targets. The cheap path has to reach the same answer as a full fold, or it is not a cheap version of it.
- [x] 3.3 Noticed because the first version of the test passed only after forcing `rebuild_projection`. That the workaround was needed *was* the finding.

## 4. Seeing it

- [x] 4.1 A redaction naming three notes queues three and destroys three — with no rebuild, so the incremental path is what is being tested.
- [x] 4.2 Exactly the cap applies to all of them.
- [x] 4.3 Over the cap queues nothing, is held, and destroys nothing.
- [x] 4.4 Separate single-target redactions still work, which is the guard that the new key did not break every redaction this build actually issues.
- [x] 4.5 **Both halves verified non-vacuous separately**, each with the sabotage confirmed to have applied before the result was trusted.

## 5. Not done

- [ ] 5.1 Nothing yet emits a multi-target redaction. The mechanism is correct for when something does; building the surface that wants it — redact an entity, redact a day — is its own change with its own confirmation design.
