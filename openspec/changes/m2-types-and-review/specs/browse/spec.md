## ADDED Requirements

### Requirement: The note page shows the structured half beside the text

The note page SHALL show the captured text verbatim together with the current
fields and the regenerated descriptor, so that drift between what was captured
and what the note now is remains visible.

#### Scenario: Both halves are on screen
- **WHEN** a promoted note is opened
- **THEN** the captured text and the current fields are both shown

#### Scenario: A suggestion below the floor is offered, not applied
- **WHEN** a note carries a recognition below the confidence floor
- **THEN** it is shown as a single line offering the interpretation
- **AND** nothing has been applied to the note

### Requirement: The calendar renders all-day events apart from the timed grid

All-day events SHALL render in a separate band above the timed grid, because
they are floating dates with no position on an hour axis. The timed grid SHALL
draw expanded occurrences with overlap resolved by column packing.

#### Scenario: A vacation is not placed on the hour axis
- **WHEN** an all-day event is displayed
- **THEN** it appears in the all-day band

#### Scenario: A DST-shifted occurrence is marked
- **WHEN** an occurrence was shifted by a DST transition
- **THEN** the calendar marks it rather than showing it silently at the shifted time

#### Scenario: A cancelled occurrence is hidden but not gone
- **WHEN** an occurrence has been cancelled
- **THEN** it is not drawn
- **AND** it remains recoverable because the cancellation is an ordinary event

### Requirement: The agenda is a view of the same substrate

The agenda SHALL present what is due and what occurs, drawn from the same notes
and occurrences as every other view, and SHALL NOT be a separate species of
object.

#### Scenario: A meeting and its notes are one record
- **WHEN** a meeting is opened from the agenda
- **THEN** the notes taken during it are on the same page
