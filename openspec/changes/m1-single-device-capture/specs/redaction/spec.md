## ADDED Requirements

### Requirement: Muting suppresses without destroying

The system SHALL provide a reversible `mute` that removes a note, tag, entity, or date range from
review and every pushed surface while leaving it readable and searchable on request.

Without it, permanent destruction would be the only way to stop seeing something — which is a bad
choice to present to someone who is upset, and an irreversible one.

#### Scenario: Muted note stops being surfaced

- **WHEN** a note is muted
- **THEN** it no longer appears in review or any pushed surface, and remains findable by search.

#### Scenario: Muting is reversible

- **WHEN** a mute is lifted
- **THEN** the note becomes eligible for surfacing again with nothing lost.

#### Scenario: Coarse mute

- **WHEN** a tag or date range is muted
- **THEN** every note matching it is suppressed without muting each individually.

#### Scenario: Timed mute

- **WHEN** a mute is set with an end date
- **THEN** the note is suppressed until that date and becomes eligible again afterwards, with no
  further action required.

### Requirement: Redaction destroys content on every replica

The system SHALL provide a `redact` operation that destroys the target's body while retaining the
event row, its identity, its body commitment, its chain link and its signature, so the log remains
gap-free and verifiable.

#### Scenario: Body is destroyed, row survives

- **WHEN** a note is redacted
- **THEN** its body is unreadable, its row remains, and `seq` continuity is unbroken.

#### Scenario: Redacted event still verifies

- **WHEN** integrity verification runs after a redaction
- **THEN** the redacted event's id recomputes and its signature verifies.

#### Scenario: Redaction is visible

- **WHEN** a redacted note is viewed
- **THEN** it renders as explicitly redacted with the date, not as a missing or blank entry.

### Requirement: Redaction defaults to the whole note

The system SHALL, by default, destroy every event targeting the note — the root capture, every
superseded version, annotations and references — and SHALL offer per-event redaction as a
non-default option.

Redacting only the current version leaves the sensitive content intact in the superseded version
that preceded it, which is the common real case.

#### Scenario: Superseded versions are destroyed

- **WHEN** a note containing a password is edited to remove it and then redacted
- **THEN** the password is not recoverable from the superseded version.

### Requirement: Redaction is scope-capped and rate-limited at ingest

The system SHALL limit a single redaction to at most 64 targets, and SHALL additionally limit the
number of targets a single origin may redact per hour, holding further redactions for explicit
confirmation once that limit is exceeded. The rate limit SHALL be enforced by the receiving
replica.

A per-event cap alone bounds nothing, because a loop emits a thousand redactions as easily as one;
and the device that will not enforce a limit on itself is exactly the faulty one.

#### Scenario: Oversized redaction is refused

- **WHEN** a redaction naming more than 64 targets is issued
- **THEN** it is refused.

#### Scenario: Runaway redaction is held

- **WHEN** a single origin has named more than 256 targets for redaction within one hour
- **THEN** further redactions from it are held pending explicit confirmation rather than executed.

### Requirement: The destruction queue is derived, not local

The system SHALL hold pending destructions in the rebuildable projection, derived from the
redaction events and their cancellations, and SHALL keep only the crash-recovery marker in
non-rebuildable local storage.

Placing the queue in local storage instead loses it on restore: the log replays, notes are marked
redacted, the projection is purged — and nothing ever destroys the bodies, because nothing is
driving the destruction. The restored replica then holds every redacted plaintext indefinitely
while reporting all redactions complete.

#### Scenario: Restore still destroys

- **WHEN** a store is restored from an export containing a redaction whose window has elapsed
- **THEN** the affected bodies are destroyed, without any local state carried over from the previous
  installation.

#### Scenario: Queue survives a rebuild

- **WHEN** the projection is rebuilt while destructions are pending
- **THEN** the pending destructions are reconstructed with their original deadlines.

### Requirement: A snapshot precedes destruction and expires

The system SHALL write a local snapshot of the affected content before issuing a redaction, SHALL
retain it for thirty days, and SHALL delete it automatically thereafter.

Redaction is the only irreversible operation in the system. The snapshot is what makes a misfire
recoverable after the cancellation window has closed — the case where you notice a week later
rather than an hour later.

The tension is real and is accepted deliberately: the snapshot is a copy of content just declared
must-not-exist. It is on by default because the realistic case is regret rather than exposure, and
because a redaction issued in error with no recovery path is the worse failure. It is local only,
never exported, and never replicated.

#### Scenario: Snapshot written before destruction

- **WHEN** a redaction is issued
- **THEN** a snapshot of the affected content exists before any body is destroyed.

#### Scenario: Snapshot expires without action

- **WHEN** thirty days pass after a snapshot is written
- **THEN** it is deleted automatically.

#### Scenario: Snapshot is not replicated or exported

- **WHEN** an export is written or a peer syncs
- **THEN** no snapshot content is included.

#### Scenario: No snapshot for credential-flagged content

- **WHEN** a redaction targets a note flagged as credential-shaped at capture
- **THEN** no snapshot is written, because a thirty-day plaintext copy is the wrong default for the
  case where the redaction is most likely deliberate.

### Requirement: Destruction is delayed and cancellable

The system SHALL hide a redaction target immediately on ingest, and SHALL physically destroy the
body six hours after the redaction's own timestamp rather than six hours after local receipt. A
`redact_cancel` within that window SHALL abort destruction and SHALL be delivered and retained with
the same priority as the redaction itself.

Deriving the deadline from local receipt would grant a fresh six-hour window to every newly
restored or paired device that later ingests an old redaction. Treating cancellation as ordinary
traffic would put the safety mechanism on a less reliable path than the destructive act.

#### Scenario: Content hides immediately

- **WHEN** a redaction is ingested
- **THEN** the target stops being visible at once, before any destruction occurs.

#### Scenario: Cancellation within the window

- **WHEN** a cancellation is issued and applied within six hours of the redaction's timestamp
- **THEN** destruction does not occur and the content is readable again.

#### Scenario: Old redaction destroys on ingest

- **WHEN** a redaction whose window has already elapsed is ingested by a device seeing it for the first time
- **THEN** destruction occurs immediately rather than after a fresh six hours.

#### Scenario: Late cancellation is reported

- **WHEN** a cancellation arrives after destruction has occurred
- **THEN** the system records and reports that the content could not be restored, rather than silently discarding the cancellation.

### Requirement: Redaction purges the projection in a crash-safe order

The system SHALL, when applying a redaction, write a durable intent record, then purge projection
rows and search-index entries, then destroy the body and reclaim storage, then clear the intent. On
restart an outstanding intent SHALL be replayed.

The event store and the projection are separate databases and cannot be committed atomically
together, so the order is chosen such that every interruption leaves the safe state: the index is
destroyed before the content is.

#### Scenario: Search index no longer matches

- **WHEN** a note containing a distinctive string is redacted
- **THEN** searching for that string returns nothing.

#### Scenario: Crash mid-purge is recovered

- **WHEN** the process crashes between purging the index and destroying the body
- **THEN** the purge is completed on restart from the intent record.

#### Scenario: No window leaves searchable text pointing at destroyed content

- **WHEN** the process is interrupted at any point during the sequence
- **THEN** there is no resulting state in which the index still matches content that has already been destroyed.

### Requirement: Redaction states its limits honestly

The system SHALL apply retained redactions when writing new export files, and SHALL state plainly
that existing exports, offsite copies, and text copied into other notes are beyond its reach, as is
guaranteed erasure on flash storage.

#### Scenario: New exports exclude redacted content

- **WHEN** an export is written after a redaction
- **THEN** the redacted content is not present in it.

#### Scenario: Prior exports are flagged, not silently ignored

- **WHEN** a redaction is applied and prior export files exist
- **THEN** the user is told those files must be pruned by hand.
