## ADDED Requirements

### Requirement: Export is incremental and append-only

The system SHALL export events written since the previous run into dated files in an export
directory, using the same canonical encoding as the log.

Because the log is append-only and immutable, an export can never be internally inconsistent and
nothing needs to be quiesced or locked.

#### Scenario: Only new events are exported

- **WHEN** an export runs after a previous export
- **THEN** it writes only events appended since that run.

#### Scenario: Export does not block capture

- **WHEN** an export runs while captures are being made
- **THEN** captures proceed normally and the export is internally consistent.

### Requirement: Credential-flagged notes are excluded from export, visibly

The system SHALL exclude notes flagged as credential-shaped at capture from export by default, SHALL
report how many were excluded, and SHALL allow the flag to be cleared on a note in one action, after
which it is included in the next export.

An export directory is meant to be copied offsite by external tools, where redaction can never reach
it. Propagating a credential there is the failure redaction exists to prevent, arriving by a route
redaction cannot follow.

The visibility requirement is not decoration. The predicate deliberately over-flags, so silent
exclusion would mean ordinary notes quietly absent from every backup — trading a leak for the data
loss this system treats as the cardinal sin. The default protects the credential case; the report
and the override protect the false-positive case.

#### Scenario: Flagged note is not exported

- **WHEN** an export runs and some notes are credential-flagged
- **THEN** those notes are not written to the export files.

#### Scenario: Exclusion is reported

- **WHEN** an export excludes flagged notes
- **THEN** the count is reported and the affected notes are listable.

#### Scenario: Flag can be cleared

- **WHEN** the user clears the flag on a note the predicate matched in error
- **THEN** that note is included in the next export.

### Requirement: Restore replays through the ordinary ingest path

The system SHALL restore by ingesting exported events through the same code path used for any other
incoming events, rather than through a special-purpose import.

A restore path exercised only during a disaster is a path that has never been tested when it
matters.

#### Scenario: Restored store matches the original

- **WHEN** a store is restored from a complete set of exports
- **THEN** the resulting projection matches what the original produced.

#### Scenario: Restore applies retained redactions

- **WHEN** an export predating a redaction is restored
- **THEN** the redaction is applied to the restored events and the content does not reappear.

### Requirement: Restore mints a fresh incarnation before any append

The system SHALL mint a new random incarnation, and therefore a new origin, before permitting any
local append following a restore.

A restored replica whose sequence counter has rolled back will otherwise reuse positions that peers
already hold, which loses both the original event and the new one, in both directions, silently.

#### Scenario: Restored device authors under a new origin

- **WHEN** a restore completes and the user captures
- **THEN** the capture is authored under a new origin at `seq` 1.

#### Scenario: Append before incarnation is refused

- **WHEN** an append is attempted after a restore but before a new incarnation exists
- **THEN** the append does not proceed and the capture is spooled instead.

### Requirement: A restored origin is bound to the roster key that owns it

The system SHALL, on minting a new origin, emit a self-signed origin claim binding that origin to
the device's existing roster public key, before authoring any other event under it.

Roster membership is keyed by public key, not by origin, precisely so that a restored device
remains a member. But nothing else in the log states which key a new origin belongs to. Without the
claim, the restored device's events reference an origin that maps to no key — and every replica
that later sees them treats them as unauthenticated and refuses them permanently. Nothing detects
this locally, because local verification uses the local key; the failure surfaces only once a second
device exists, in events that are immutable by then.

#### Scenario: Claim precedes ordinary events

- **WHEN** a restore mints a new origin
- **THEN** the first event authored under it is an origin claim, self-signed by the device key.

#### Scenario: Claim is verifiable against the roster

- **WHEN** an origin claim is ingested
- **THEN** its signature verifies against a public key already on the roster, and the origin becomes
  a recognised author.

#### Scenario: Events under an unclaimed origin are not folded

- **WHEN** events arrive under an origin with no claim and no roster binding
- **THEN** they are not folded, and the condition is surfaced rather than passing silently.

### Requirement: Recovering from a mistaken redaction is explicit

The system SHALL provide a restore mode that ignores redactions issued after a stated timestamp,
and SHALL NOT make this the default.

Ordinary restore must apply redactions, or restoring an old backup would resurrect content
deliberately destroyed. But then plain restore cannot recover from a redaction fired at the wrong
target, so the two intents are separated rather than conflated.

#### Scenario: Deliberate recovery of wrongly redacted content

- **WHEN** a restore is run with an explicit cutoff predating a mistaken redaction
- **THEN** the affected content is restored and the redactions after that cutoff are not applied.

#### Scenario: Default restore is unaffected

- **WHEN** a restore is run without the flag
- **THEN** every retained redaction applies.

### Requirement: Replication is not treated as backup

The system SHALL make clear that holding the same data on several devices does not protect against
a defect that writes wrong events, nor against every device being lost together, and SHALL rely on
external tools for offsite copies of the export directory.

#### Scenario: Export directory is external-tool friendly

- **WHEN** an external backup tool is pointed at the export directory
- **THEN** it can copy it without coordination with the running application.

#### Scenario: Offsite copying is out of scope

- **WHEN** the user asks the system to send backups to a remote destination
- **THEN** the system does not do so and documents the external-tool approach instead.
