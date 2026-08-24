## ADDED Requirements

### Requirement: The fold takes the event set and nothing else

The system SHALL derive all projected state from the event set alone. The local wall clock, the
roster as it currently stands, and any other ambient input SHALL NOT influence the result.

Two earlier approaches violated this and both produced the same failure: the same log rendering
differently on different machines, or on the same machine on different days.

#### Scenario: Shuffled input yields identical state

- **WHEN** the same event set is folded in any two arrival orders
- **THEN** the fold's own output value is byte-identical under its canonical encoding.

#### Scenario: The stored projection is logically equivalent

- **WHEN** the same event set is folded in any two arrival orders and written to storage
- **THEN** every query returns the same results, though the database files are not byte-identical —
  auto-assigned row identifiers and index layout depend on insertion order and are not part of the
  fold's meaning.

#### Scenario: Time-dependent state is not stored

- **WHEN** an occurrence has passed without completion, or a completion rate is requested
- **THEN** these are computed at read time from the current clock and are not columns in the projection.

### Requirement: The projection is disposable and rebuilt by re-folding

The system SHALL treat `state.db` as a cache that can be deleted and reconstructed from the log at
any time, and SHALL use re-folding as the entire schema-migration strategy. No migration code SHALL
exist.

#### Scenario: Schema version mismatch triggers rebuild

- **WHEN** the application starts and the projection schema version does not match the binary's
- **THEN** the projection is rebuilt by re-folding, with no data-migration step.

#### Scenario: Corrupt projection recovers

- **WHEN** `state.db` is deleted or corrupted
- **THEN** a rebuild reproduces the previous state exactly from `events.db`.

#### Scenario: Rebuild does not block reading

- **WHEN** a rebuild is in progress
- **THEN** the existing projection remains readable until the new one is complete and is then swapped.

### Requirement: The fold is incremental per origin and per note

The system SHALL track a fold watermark per origin, not as a single scalar, and SHALL re-fold only
the notes affected by newly ingested events.

Events arrive out of timestamp order by construction, so a single watermark either skips late
arrivals or forces a full re-fold on every reconnection.

#### Scenario: Late-arriving event is folded

- **WHEN** an event with an older timestamp than the current watermark is ingested
- **THEN** it is folded and the notes it touches are recomputed.

#### Scenario: Unrelated notes are untouched

- **WHEN** a single event is ingested
- **THEN** only the notes it targets are recomputed.

### Requirement: Notes carry head state and capture provenance

The system SHALL materialise, for each note: its type, current text, rendered descriptor, winning
head event, creation and update timestamps, mute state, pinned state, redaction date, divergence
flag, and the IANA timezone in which it was captured.

The capture timezone cannot be recovered later and is required for day grouping to remain correct
when the user travels.

#### Scenario: Capture timezone is recorded

- **WHEN** a note is captured at 23:40 local time in one zone and viewed on a device in another
- **THEN** it is grouped under the day it was captured in, not the viewing device's day.

#### Scenario: Descriptor reflects structured fields

- **WHEN** a note's structured fields change
- **THEN** the descriptor is regenerated while the captured text remains verbatim.

### Requirement: The version graph records edges, including quarantined ones

The system SHALL materialise a per-note version graph containing every version event, its
predecessor, its kind, and whether it is quarantined.

Concurrency detection is reachability through the predecessor chain and requires edges to walk.
Omitting quarantined events would dangle their successors' edges and report legitimate sequential
edits as divergences.

#### Scenario: Sequential edits are not divergences

- **WHEN** two edits are made in sequence, the second having seen the first
- **THEN** the second is reachable from the first and no divergence is recorded.

#### Scenario: Multi-edit branch versus single edit

- **WHEN** two edits are made on one device and one on another, all from the same starting version
- **THEN** neither branch is reachable from the other and exactly one divergence is recorded.

#### Scenario: Edge survives quarantine

- **WHEN** a version event is quarantined and a later event names it as predecessor
- **THEN** the edge is retained and the walk still reaches the root.

### Requirement: Divergences retain both versions and are dismissible by event

The system SHALL retain both competing versions when a genuine divergence is detected, SHALL
surface the count, and SHALL clear a divergence only in response to a `resolve` event.

Local dismissal state would be resurrected by every re-fold; without any dismissal the count would
never clear.

#### Scenario: Both versions survive

- **WHEN** two concurrent edits are folded
- **THEN** the timestamp winner becomes current and the loser is retained and viewable.

#### Scenario: Dismissal persists across rebuild

- **WHEN** a divergence is dismissed and the projection is rebuilt
- **THEN** it remains dismissed.

### Requirement: The fold orders events by the hybrid logical clock

The system SHALL fold events in HLC order, using the total order defined in the event-log
capability, so that every replica computes the same result from the same set regardless of the
order events arrived in.

#### Scenario: Arrival order does not affect the fold

- **WHEN** the same events are ingested in different orders on two occasions
- **THEN** the folded state is equivalent, because both folds sort by HLC before applying.

### Requirement: Conflicts resolve by type, and every kind has a rule

The system SHALL resolve competing events by the following rules, and SHALL NOT leave any event
kind it writes without one:

| Situation | Rule |
|---|---|
| Concurrent note bodies | both retained; HLC winner current, loser flagged |
| Tags | add-wins observed-remove set |
| Scalars — pin, task state, due date | last-writer-wins by HLC, prior value retained in history |
| `alias` | last-writer-wins by HLC |
| Delete versus edit | **edit wins**; the tombstone is retained but does not apply |
| `mute` versus `mute` | last-writer-wins by HLC |
| Redaction versus anything | **redaction wins, at any HLC, in any arrival order** |

Losing an edit is worse than keeping a note the user meant to delete, so deletion yields. Access and
destruction go the other way.

#### Scenario: Edit beats a concurrent delete

- **WHEN** a note is deleted on one occasion and concurrently edited
- **THEN** the note survives with the edit applied, and the tombstone is retained without effect.

#### Scenario: Scalar conflict keeps history

- **WHEN** the same scalar field is set twice concurrently
- **THEN** the HLC winner is current and the losing value remains inspectable.

### Requirement: Redaction wins regardless of order

The system SHALL retain every redaction indefinitely and SHALL apply it to any matching event that
arrives later, from any source, including events that arrive for the first time long afterwards.

Without indefinite retention a replica holding an older copy reintroduces destroyed content the
moment it is heard from, and redaction is not a guarantee but a race.

#### Scenario: Redaction arrives before its target

- **WHEN** a redaction is folded and the event it names is ingested afterwards
- **THEN** the content is destroyed on arrival rather than becoming readable.

#### Scenario: Redaction outlives a rebuild

- **WHEN** the projection is rebuilt from the log
- **THEN** every retained redaction still applies.

### Requirement: The roster is available to ingest, and ingest waits for it

The system SHALL materialise the roster — public keys, their origins, and their status — as part of
the projection, and SHALL pause ingest rather than accept unverified events while the roster is
being rebuilt.

Ingest verifies every event against its origin's key, so the roster is not merely a view; it is an
input to accepting data. Because the projection can be dropped and rebuilt at any time, the window
in which it is unavailable has to be defined rather than discovered.

#### Scenario: Ingest pauses during rebuild

- **WHEN** the projection is being rebuilt and events arrive
- **THEN** they are not accepted as verified until the roster is available.

#### Scenario: Roster survives rebuild

- **WHEN** the projection is rebuilt
- **THEN** the roster is reconstructed from the log with the same contents.

### Requirement: Slugs are deterministic, frozen, and collision-resolved

The system SHALL derive a canonical slug for every note from its root capture body plus a 50-bit
suffix of the note identity. The derivation SHALL be byte-specified: lowercase, NFC normalisation,
Unicode word segmentation, the first four words longer than three characters, ASCII transliteration
where the script has a standard one and the raw word otherwise, hyphen-joined. Collisions SHALL be
resolved deterministically by note-identity ordering rather than by arrival order.

The Unicode tables used for normalisation, segmentation and transliteration SHALL be pinned, and
their version SHALL form part of the slug algorithm version. A dependency upgrade that changed
segmentation would change existing slugs on the next rebuild and break every hand-written link —
which is the failure freezing the algorithm exists to prevent, arriving through the back door.

Deriving from the current text or from machine-generated titles would change slugs when enrichment
runs, with the same consequence.

#### Scenario: Derivation matches its test vectors

- **WHEN** the slug algorithm runs against the committed vector file
- **THEN** every output matches byte for byte.

#### Scenario: Unicode table version is part of the algorithm version

- **WHEN** the pinned Unicode tables are updated
- **THEN** the slug algorithm version changes, and the change is treated as a new algorithm rather
  than an in-place upgrade.

### Requirement: Captures that look like credentials do not get word-derived slugs

The system SHALL evaluate every capture against a pure credential predicate — high-entropy tokens,
long base64 or hexadecimal runs, `password`/`token`/`secret`/`key` adjacent to a value, PEM headers,
connection strings — and SHALL give a flagged capture a date-derived slug rather than a word-derived
one. The predicate SHALL be part of the frozen slug algorithm and SHALL NOT depend on the
recogniser.

A slug is the one part of a note's content that is copied into *other* notes, as link text, where
redaction cannot reach it. So a credential-shaped slug is a leak with no cleanup path. The predicate
must ship with the frozen algorithm because adding it later would change existing slugs on the next
rebuild — the breakage freezing exists to prevent.

The predicate SHALL be tuned to over-flag: a false positive costs readability, a false negative is
permanent.

#### Scenario: Credential-shaped capture gets a date slug

- **WHEN** a capture contains a high-entropy token or a credential keyword adjacent to a value
- **THEN** its slug derives from its kind and date, not from its words.

#### Scenario: Ordinary capture is unaffected

- **WHEN** a capture contains no credential indicators
- **THEN** its slug derives from its words as normal.

#### Scenario: Tightening the predicate does not rewrite history

- **WHEN** a later version flags a pattern the earlier version did not
- **THEN** notes already slugged by words keep their existing slugs, and only new captures are
  affected.

#### Scenario: Slug is stable across edits and rebuilds

- **WHEN** a note is edited, enriched, and its projection rebuilt
- **THEN** its slug is unchanged.

#### Scenario: Identical captures get distinct slugs

- **WHEN** the same short text is captured many times
- **THEN** each note receives a distinct slug and the fold completes without constraint failure.

#### Scenario: Textless capture still gets a readable slug

- **WHEN** a capture has no text at capture time
- **THEN** the slug uses a kind-and-date prefix plus the suffix, and never derives from later machine output.

#### Scenario: Redacted note gets a text-free slug

- **WHEN** a note is redacted
- **THEN** its slug becomes a form derived from the note identity alone, so it reveals nothing and the row is reproducible on re-fold.

### Requirement: Tags are an add-wins observed-remove set

The system SHALL record tag additions individually and SHALL permit a removal to retract only the
additions it observed.

#### Scenario: Concurrent add survives a remove

- **WHEN** a tag is removed on one device while concurrently added on another
- **THEN** the tag is present after both are folded.

#### Scenario: Re-adding after removal

- **WHEN** a tag is removed and later re-added
- **THEN** the tag is present.

### Requirement: Search indexes current text and is purgeable

The system SHALL maintain a full-text index over note text, titles and transcripts, keyed so that
any individual note's entries can be resolved back to it and deleted.

The index must cover the current head rather than the root capture, or nothing added in an edit
would ever be findable; and it must support deletion, or redaction cannot purge it.

#### Scenario: Text added in an edit is findable

- **WHEN** a note is edited to add a word and the index updates
- **THEN** searching for that word returns the note.

#### Scenario: Search hit resolves to a note

- **WHEN** a search matches
- **THEN** the result identifies the note it belongs to.

#### Scenario: Index entry can be deleted for one note

- **WHEN** a single note's index entries are purged
- **THEN** other notes' entries are unaffected and the note no longer matches.
