## ADDED Requirements

### Requirement: A superseded generation does not accumulate

The store SHALL remove projection generations it has superseded. A rebuild
writes a new generation beside the current one and moves the pointer, which
leaves the old file intact deliberately — readers keep the file they already
opened — but it SHALL NOT be left there for ever.

The current generation and the one immediately before it SHALL be kept.
Anything older SHALL be removed. Keeping one spare is what makes the gap
between a reader resolving the pointer and opening the file safe, without
requiring a read to take a lock (§8.8).

Removal SHALL happen while the writer lock is held, and a removal that fails
SHALL NOT fail the operation it happened during. Disk that could not be
reclaimed is disk to reclaim next time, not a reason to refuse the store.

#### Scenario: Older generations are removed
- **WHEN** a store has been rebuilt more than once
- **AND** a session is opened
- **THEN** generations older than the one before the current one no longer exist

#### Scenario: The current generation and one spare survive
- **WHEN** generations are removed
- **THEN** the current generation still exists
- **AND** the generation immediately before it still exists

#### Scenario: A rebuild does not strand the generation it replaced
- **WHEN** a projection is rebuilt
- **THEN** the generation it replaced is still readable by anything that had resolved it

#### Scenario: Removal that fails is not fatal
- **WHEN** a superseded generation cannot be removed
- **THEN** the session still opens
