## ADDED Requirements

### Requirement: The window corrects a kind where it shows one

The window SHALL offer to make a capture a task, and to demote a promoted
capture to a plain note, from the surface that renders what it is.

The requirement that a wrong kind is correctable where it is read was written
against every surface, and the window had the half that comes from the
recognizer — accepting a suggestion below the floor — and neither of the halves
that come from a person. Saying "this is a task" is the ordinary correction,
because a bare task verb is below the floor by design, so most tasks arrive by
somebody saying so.

A correction SHALL be pinned, and the captured text SHALL be unchanged.

Naming a task and dating it SHALL remain separate: the due date SHALL be
optional, because deciding something is owed and deciding when it is owed are
different decisions and one often arrives without the other.

#### Scenario: Saying a note is a task, from the window
- **WHEN** a plain note is made a task in the window
- **THEN** it SHALL become one
- **AND** the captured text SHALL be unchanged

#### Scenario: With a due date, and without
- **WHEN** a note is made a task and no due date is given
- **THEN** it SHALL be an undated task
- **AND** giving one SHALL set it

#### Scenario: Demoting from the window
- **WHEN** a promoted capture is made a plain note in the window
- **THEN** it SHALL be a plain note
- **AND** the captured text SHALL be unchanged

#### Scenario: The correction outlives a reparse
- **WHEN** a kind has been corrected in the window
- **AND** the capture is later read again under a newer grammar
- **THEN** the corrected kind SHALL stand
