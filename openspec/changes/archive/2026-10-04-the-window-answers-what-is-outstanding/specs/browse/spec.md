## ADDED Requirements

### Requirement: The window offers the task surface

The window SHALL offer a view of outstanding tasks in the three bands, and SHALL
allow a task to be completed from it.

The requirement that every first-party surface offers one was written against
all of them, and the window was the one without. The consequence is not a
missing convenience: a task occupies no time, so the agenda cannot hold it, and
the agenda's due band requires a due date — so an **undated** task, which is the
ordinary kind, was reachable in the window only by scrolling a timeline ordered
by when things were captured.

Completing a task from this surface SHALL be the same write the other surfaces
make. Two paths to one state is how two surfaces come to disagree about it.

#### Scenario: The window shows the three bands
- **WHEN** tasks are outstanding in more than one band
- **THEN** the window SHALL show each band, labelled, with the undated band present

#### Scenario: An undated task is reachable in the window
- **WHEN** a task carries no due date
- **THEN** it SHALL appear in the window's undated band

#### Scenario: Completing from the window
- **WHEN** a task is marked done in the window
- **THEN** it SHALL leave the bands
- **AND** the write SHALL be the one the command line makes

#### Scenario: Nothing outstanding reads as nothing, not as a fault
- **WHEN** no task is open
- **THEN** the window SHALL say so plainly
