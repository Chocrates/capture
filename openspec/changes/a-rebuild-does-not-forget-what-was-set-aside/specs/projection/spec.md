## ADDED Requirements

### Requirement: A rebuilt projection reports what the log has set aside

A rebuild SHALL repopulate the record of set-aside events from the log, which
holds the authoritative mark. The projection is derived and disposable (§1.3),
so anything it is the only copy of is lost whenever it is rebuilt — and the set
of events this device is refusing to use is not something a surface may be
wrong about.

#### Scenario: Set-aside events survive a rebuild
- **WHEN** events have been set aside
- **AND** the projection is rebuilt
- **THEN** they are still reported as set aside
- **AND** the reason recorded for each is unchanged

#### Scenario: A reissued retirement still re-admits after a rebuild
- **WHEN** events are held because a retirement excluded them
- **AND** the projection is rebuilt
- **AND** the retirement is reissued with a later head
- **THEN** the events below that head are re-admitted

#### Scenario: Nothing set aside stays nothing
- **WHEN** no event has been set aside
- **AND** the projection is rebuilt
- **THEN** nothing is reported as set aside

### Requirement: A rebuilt record is ordered by when the event was authored

Where a set-aside record is restored by a rebuild, the moment it carries SHALL
be the event's own timestamp. When this device first saw the event is not in the
log — it existed only in the projection being replaced — and the authored
instant is stable across rebuilds, so the report does not reorder itself each
time one happens.

#### Scenario: The order does not change between rebuilds
- **WHEN** a projection holding set-aside records is rebuilt twice
- **THEN** the records are reported in the same order both times
