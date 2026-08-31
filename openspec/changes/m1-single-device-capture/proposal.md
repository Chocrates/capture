## Why

Nothing is built. `DESIGN.md` specifies a full peer-to-peer system across five milestones, and the
temptation is to build the interesting part — the sync protocol — first. M1 deliberately does not:
it delivers a single-device system that is genuinely usable every day, on the durable event log the
rest of the design depends on.

Two reasons for that order. The durable layer is the part that cannot be revised later — an event
written under a wrong identity rule is wrong forever — so it must be right before a second device
exists to disagree with it. And the project's central open question (`DESIGN.md` §9 Q0) is whether
this tool helps at all; M1 is the cheapest honest attempt to find out, and it needs no protocol
work to answer.

## What Changes

- **A durable, append-only event log** (`events.db`) with the identity, signing, and integrity
  rules that the whole design rests on: envelope-covering ids, nonce-salted body commitments,
  contiguous version-vector heads, and a strict separation between events that fail authentication
  and events that are merely not yet understandable.
- **A rebuildable projection** (`state.db`) folded from the log, holding notes, the note version
  graph, tags, entities, ambient context, links and full-text search. Disposable by construction:
  schema changes are handled by re-folding, never by migration code.
- **Capture** as a single verb with no decisions — a resident overlay bound to a global hotkey, a
  CLI path, and an ambient envelope collected automatically. Capture never blocks on disk or
  network, and degrades to a preallocated spool rather than failing.
- **Redaction and muting** — the only irreversible operation in the system, scope-capped,
  rate-limited at ingest, cancellable for six hours, and ordered so no crash window ever leaves
  searchable text pointing at destroyed content. Muting exists so that destruction is never the
  only way to stop seeing something.
- **Export and restore** — incremental event export for external backup tools, and a restore path
  that mints a fresh origin incarnation so a restored replica can never reuse sequence positions.
- **Browse** — timeline, note page with backlinks, full-text search, and a health surface that
  reports the system's state honestly rather than optimistically.
- **A single `capture` CLI** that is both a capture path and the integration test harness, including
  `capture verify` for the integrity claims and `--json` on every read command.
- **First run** — four startup states (not three), identity generation, roster genesis, and hotkey
  registration that is *confirmed* rather than assumed.

No breaking changes: there is nothing yet to break.

## Capabilities

### New Capabilities

- `event-log`: the durable append-only log — canonical encoding, event identity and signing, body
  commitments, append and sequencing, contiguous heads, origin incarnation, the quarantine/rejected
  split, forward-compatible schema evolution, and integrity verification.
  (`DESIGN.md` §1, §1.3, §5.3 ingest rules)
- `projection`: folding the log into `state.db` — note head state, the version graph and
  concurrency detection, slug derivation, tags as an OR-Set, entities, ambient context, FTS, and
  rebuild-by-re-fold as the entire migration strategy. (`DESIGN.md` §1.4, §6, §4.3 slugs)
- `capture`: the capture verb and its guarantees — one gesture, zero decisions, latency target,
  ambient envelope, never blocking, disk-full spool. (`DESIGN.md` §2.1, §2.2, §2.3, §7 modes 1–6)
- `redaction`: `redact`, `redact_cancel` and `mute` — scope cap, ingest-side rate limit, HLC-derived
  destruction deadline, ordered cross-database purge, and the honest limits of what destruction can
  reach. (`DESIGN.md` §1.1, §2.6)
- `export-restore`: incremental export for external backup tools, restore as replay, fresh
  incarnation on restore, and the deliberate `--ignore-redactions-after` escape.
  (`DESIGN.md` §1.2, §1 incarnation)
- `browse`: timeline, note page, links and backlinks, full-text search, and the three-tier health
  surface. (`DESIGN.md` §4.1, §4.2, §4.3, §4.5)
- `cli`: the single `capture` binary — capture that survives shell metacharacters, read/write/ops
  commands, `--json`, exit codes, and `capture verify`. (`DESIGN.md` §8.6)
- `first-run`: startup state detection, identity and incarnation, roster genesis, and hotkey
  registration with confirmation and dead-key detection. (`DESIGN.md` §8.7)

### Modified Capabilities

None. This is the first change; `openspec/specs/` is empty.

## Impact

- **New cargo workspace**: `crates/capture-core` (I/O-free, async-free — the fold and the conflict
  rules live here so they are exhaustively testable without a runtime), `crates/capture-store`
  (SQLite, both databases), `apps/capture-cli`, `apps/capture-overlay` (egui overlay),
  `apps/capture-desktop` (Tauri, timeline and note page only).
- **Dependencies**: `rusqlite` (bundled SQLite, FTS5, `contentless_delete`), `blake3`,
  `ed25519-dalek`, deterministic CBOR, `jiff` for the capture timezone. No async runtime in M1 —
  `tokio` arrives with sync in M3.
- **Platform integration**: global hotkey registration on Linux (XDG `GlobalShortcuts` portal under
  Wayland, X11 grab otherwise) and macOS (Accessibility permission). This is the highest-risk item
  in the milestone and is called out in `DESIGN.md` §8.5.
- **Frozen decisions** that later milestones inherit and cannot revise: the canonical encoding, the
  event identity construction, the slug derivation algorithm, and origin identity. Getting these
  wrong is not recoverable by a later change.

## Non-goals

Deliberately deferred, each to a named milestone:

- **Sync (BSP), pairing, anti-entropy, gossip, blob plane** — M3. M1 has one device and one origin;
  the log format is designed for replication but nothing replicates yet.
- **Android** — M4.
- **Enrichment** — M5. Transcription, entity extraction, link suggestion, the specificity pass. An
  un-enriched capture is a complete capture; nothing in M1 depends on enrichment existing.
- **Recognizer promotion, tasks, events, recurrence, calendar** — M2. M1 captures notes; typed
  promotion arrives with `capture-time` and `capture-parse`.
- **Scheduled review, intents, activation** — M2. These are the evidence-backed components and they
  ship in M2 rather than last (`DESIGN.md` §8.8), but they need the recurrence machinery M2 builds.
- **At-rest encryption, app lock, expiry wipe, panic redact** — deferred as a cluster
  (`DESIGN.md` §9.14). M1 stores plaintext on disk and says so.
- **Voice capture** — the overlay accepts text in M1. Audio blobs need the blob plane (M3) and
  transcription (M5) to be useful rather than a write-only hole.
