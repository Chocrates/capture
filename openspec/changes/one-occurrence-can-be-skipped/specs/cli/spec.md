## ADDED Requirements

### Requirement: One occurrence of a series can be skipped

The command line SHALL be able to cancel a single occurrence of a recurring
series, leaving the rule and every other occurrence untouched. The occurrence
SHALL be named by its original start, as the series' other exception commands
already name one.

This is the gesture for "not this Thursday". Editing the rule changes every
future occurrence, and leaving the entry standing shows a meeting that is not
happening — so without this the two available answers are both wrong.

#### Scenario: Skipping one occurrence
- **WHEN** one occurrence of a series is skipped
- **THEN** that occurrence is marked cancelled
- **AND** the series' rule is unchanged
- **AND** every other occurrence is unaffected

#### Scenario: Skipping what is already skipped
- **WHEN** an occurrence that is already cancelled is skipped again
- **THEN** it is not an error
- **AND** the occurrence is still cancelled

#### Scenario: A series that does not exist
- **WHEN** a note that is not a series is given
- **THEN** it is refused, naming what was expected

### Requirement: A skipped occurrence can be restored

The command line SHALL be able to restore a skipped occurrence. Restoring SHALL
be expressed as moving the occurrence back to its own original start, because a
move beats a cancellation and no un-cancel event exists.

That asymmetry is deliberate and SHALL NOT be removed by adding one: a
resurrected occurrence is visible and one gesture away, where a wrongly
cancelled one is invisible and silently missed.

#### Scenario: Restoring a skipped occurrence
- **WHEN** a skipped occurrence is restored
- **THEN** it appears again at its original time
- **AND** the cancellation remains in the log

#### Scenario: Restoring one that was never skipped
- **WHEN** an occurrence that is not cancelled is restored
- **THEN** it is not an error
- **AND** the occurrence still appears at its original time
