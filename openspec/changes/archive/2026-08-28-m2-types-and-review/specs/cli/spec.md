## ADDED Requirements

### Requirement: The command line can read the derived types

The CLI SHALL be able to list the agenda, tasks and occurrences, and SHALL
render the structured fields alongside the captured text. Every such command
SHALL take `--json`.

#### Scenario: The agenda is available in a terminal
- **WHEN** the agenda is requested for a day
- **THEN** the timed occurrences and all-day entries for that day are listed

#### Scenario: Machine-readable output is available
- **WHEN** any of these commands is given `--json`
- **THEN** the output is JSON

### Requirement: A promotion can be corrected from the command line

The CLI SHALL be able to correct a promotion, and a correction made this way
SHALL be pinned so that no later reparse overrides it.

#### Scenario: A correction survives a reparse sweep
- **WHEN** a promotion is corrected from the command line and the backlog is later reparsed
- **THEN** the correction still governs

### Requirement: Completing an occurrence does not require the GUI

The CLI SHALL be able to complete and un-complete a single occurrence of a
recurring obligation, naming it by series and original start.

#### Scenario: An instance is completed by name
- **WHEN** a single occurrence is completed from the command line
- **THEN** only that occurrence is complete
- **AND** the series is unaffected
