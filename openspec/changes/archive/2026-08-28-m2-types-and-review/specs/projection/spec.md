## ADDED Requirements

### Requirement: Occurrences are expanded into the projection and are rebuildable

The projection SHALL expand recurring occurrences over a rolling horizon,
extended in the background and on view access. Expanded occurrences SHALL be
derived data and SHALL be discarded and recomputed by a rebuild.

#### Scenario: A rebuild reproduces the expansion exactly
- **WHEN** the projection is rebuilt
- **THEN** the expanded occurrences match those the previous generation held

#### Scenario: A timezone database update needs no migration
- **WHEN** the system timezone database changes
- **THEN** the correct expansion is obtained by re-folding
- **AND** the log and the stored instants are untouched

### Requirement: The due queue carries review and intent deadlines

The due-work queue SHALL carry scheduled review sessions and time-based intent
deadlines alongside redaction deadlines, and SHALL be processed by the same
mechanism: every process startup is the floor, with a resident window making a
deadline land near its time.

#### Scenario: A deadline that fell while nothing ran is handled at startup
- **WHEN** a review or intent deadline elapsed while no process was running
- **THEN** it is processed at the next startup
- **AND** nothing is lost because no timer was running

### Requirement: Promoted fields are queryable without reading the text

The projection SHALL store the structured fields of a promoted note and its
descriptor so that agenda, calendar and task queries do not parse text at read
time.

#### Scenario: The agenda is a query
- **WHEN** the agenda for a day is requested
- **THEN** it is answered from stored fields and expanded occurrences
