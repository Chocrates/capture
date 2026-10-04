## MODIFIED Requirements

### Requirement: The note page shows the structured half beside the text

The note page SHALL show the captured text verbatim together with the current
fields and the regenerated descriptor, so that drift between what was captured
and what the note now is remains visible.

This SHALL hold on every surface that has a note page, not only the desktop. A
phone that showed the text alone would be a second answer to "what is this
note", and the one most people actually read.

#### Scenario: Both halves are on screen
- **WHEN** a promoted note is opened
- **THEN** the captured text and the current fields are both shown

#### Scenario: Both halves are on screen, on every surface
- **WHEN** a promoted note is opened on the phone
- **THEN** the captured text and the current fields are both shown

#### Scenario: A suggestion below the floor is offered, not applied
- **WHEN** a note carries a recognition below the confidence floor
- **THEN** it is shown as a single line offering the interpretation
- **AND** nothing has been applied to the note

## ADDED Requirements

### Requirement: A row renders what the capture is, not only what was typed

A row in a reading surface SHALL render the promotion the capture carries — a
task with its state, an event with its time, a series with its rule, a list
with its items — and SHALL show the captured text alongside it.

A capture carrying no promotion SHALL render as its text alone. A note's
content is its text, and a descriptor invented for one would add a fact the
capture did not contain (§3.1.3).

The captured text SHALL NOT be replaced by the rendering. §3.1.4 requires the
text, the fields and the descriptor to be visible together so that a
disagreement between them can be seen; a row showing only the fields would hide
the disagreement the descriptor exists to reveal.

#### Scenario: A task reads as a task
- **WHEN** a capture promoted to a task with a due date appears in a timeline
- **THEN** the row shows that it is a task and when it is due
- **AND** the captured text is still shown

#### Scenario: An ordinary note is unchanged
- **WHEN** a capture carrying no promotion appears in a timeline
- **THEN** the row shows its text
- **AND** no descriptor is invented for it

#### Scenario: Drift stays visible
- **WHEN** a capture reading "every day at 7" carries fields saying weekdays
- **THEN** both the text and the rendered rule are on the row

### Requirement: A list renders as a list wherever it appears

A capture recognised as a list SHALL render as its items, each showing whether
it has been completed, and each completable in place. This SHALL NOT require
navigating to a separate surface first.

A tag-wide list view gathers items across several captures and remains
available; it is not the only way to reach the items of one capture.

#### Scenario: A list capture is a list on sight
- **WHEN** a list capture appears in a timeline
- **THEN** its items are shown with their completion state

#### Scenario: An item is completed where it is drawn
- **WHEN** an item is completed from the row it appears on
- **THEN** it is marked done
- **AND** no navigation was required to do it

#### Scenario: The sigil is not what the reader is left with
- **WHEN** a capture reading `#groceries: milk, eggs, bread` is drawn
- **THEN** the reader is shown three items
