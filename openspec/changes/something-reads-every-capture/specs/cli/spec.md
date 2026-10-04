## ADDED Requirements

### Requirement: Reading outstanding captures can be asked for

The CLI SHALL provide a command that reads whatever captures have not been read
and reports how many were promoted. It is a write command and SHALL take the
writer lock like any other; it exists so that a person on a machine with no
daemon has a remedy to be pointed at, and so that "nothing was read" can be
distinguished from "it was read and found nothing".

#### Scenario: Outstanding captures are read on request
- **WHEN** captures have not been read and the command is run
- **THEN** they are read
- **AND** the number promoted is reported

#### Scenario: Nothing to do says so
- **WHEN** everything has already been read
- **THEN** the command reports that nothing was outstanding
- **AND** exits successfully

#### Scenario: The writer is busy
- **WHEN** another process holds the writer lock
- **THEN** the command reports that it could not take it
- **AND** does not claim to have read anything

### Requirement: A read reports what it has not read

A read command whose answer depends on promotion SHALL report any outstanding
captures alongside its answer, and SHALL name the remedy. This SHALL be done
from the read-only handle: a read SHALL NOT take the writer lock in order to
discover or to resolve staleness (§8.8).

The report is one line and SHALL NOT suppress the answer. An empty task list
with "3 captures not yet read" is a different statement from an empty task
list, and the difference is the one a person needs.

#### Scenario: An empty list that is not necessarily empty
- **WHEN** a dated task was captured and not yet read
- **AND** the task list is requested
- **THEN** the list is shown as it stands
- **AND** a line reports the outstanding captures and names the command that reads them

#### Scenario: Nothing outstanding, nothing said
- **WHEN** every capture has been read
- **THEN** no staleness line appears

#### Scenario: A read does not become a write
- **WHEN** a read command runs while the writer lock is held
- **THEN** it answers
- **AND** reports the outstanding captures

## MODIFIED Requirements

### Requirement: The command line can read the derived types

The CLI SHALL be able to list the agenda, tasks and occurrences, and SHALL
render the structured fields alongside the captured text. Every such command
SHALL take `--json`.

Where any capture has not yet been read, these commands SHALL report that
alongside their output, so that an answer derived from incomplete promotion is
never presented as a complete one.

#### Scenario: The agenda is available in a terminal
- **WHEN** the agenda is requested for a day
- **THEN** the timed occurrences and all-day entries for that day are listed

#### Scenario: Machine-readable output is available
- **WHEN** any of these commands is given `--json`
- **THEN** the output is JSON

#### Scenario: An incomplete answer says that it is incomplete
- **WHEN** captures are outstanding and any of these commands is run
- **THEN** the output reports how many captures have not been read
