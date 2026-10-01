## ADDED Requirements

### Requirement: The log gains the entity rename, merge and bind kinds

The event log SHALL accept `entity_rename`, `entity_merge` and `entity_bind`.
Each SHALL have a golden encoding vector written before anything writes one,
because the encoding is frozen from its first write.

Each SHALL name an entity by its `ent_id` rather than by a spelling, so that a
later rename cannot orphan an event that was about the entity rather than about
the word.

None of the three SHALL be inferred. An entity is created by a note naming it,
and everything else here is somebody deciding.

#### Scenario: A new kind round-trips byte for byte
- **WHEN** an event of each new kind is encoded and decoded
- **THEN** the result is identical to the input
- **AND** its id recomputes to the same value

#### Scenario: An older build holds rather than misreads
- **WHEN** a build that does not recognise one of these kinds ingests it
- **THEN** the event is held with a recorded reason
- **AND** nothing is discarded
- **AND** the event is re-evaluated after the build is upgraded

#### Scenario: An entity event is not about a note
- **WHEN** an entity event is folded
- **THEN** it SHALL apply without requiring a note to exist as its target

### Requirement: Merging is idempotent and order-independent

Two `entity_merge` events joining the same pair SHALL be equivalent to one. Two
devices merging the same pair while partitioned SHALL converge to one answer
without either being rejected.

#### Scenario: The same merge twice
- **WHEN** the same pair is merged twice
- **THEN** the result is the same as merging once

#### Scenario: Both devices merged the same pair
- **WHEN** two devices each merge A into B and then sync
- **THEN** both devices agree which entity survives
