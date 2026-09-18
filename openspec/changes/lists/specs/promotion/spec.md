## ADDED Requirements

### Requirement: A list item is not promoted by being completed

Completing a list item SHALL NOT append a promotion and SHALL NOT create a
capture. An item is a projection row expanded from its list capture, on the
same terms as a recurring occurrence, and completing one is a fact about that
row rather than a new thing in the archive.

An item SHALL therefore never appear as a task in `capture tasks`, in the
timeline, or anywhere else that lists what has been captured.

#### Scenario: Ticking creates no task
- **WHEN** a list item is completed
- **THEN** no promotion to a task is appended
- **AND** the item does not appear among the tasks

#### Scenario: The timeline holds only captures
- **WHEN** every item on a list has been completed
- **THEN** the timeline holds one entry, the list capture itself
