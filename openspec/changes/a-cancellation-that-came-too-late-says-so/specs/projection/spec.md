## ADDED Requirements

### Requirement: A cancellation arriving after destruction is kept

The projection SHALL keep, against the note, that a cancellation was folded
for a redaction already carried out.

The fold already learns both halves — that the content is gone, and that the
governing order was withdrawn — and discarded their combination. Nothing could
report what nothing kept.

#### Scenario: A cancellation arrives after the content is gone
- **WHEN** a note's redaction has been carried out
- **AND** a cancellation for that redaction is folded
- **THEN** the note records that a cancellation arrived too late

#### Scenario: A cancellation inside the window is not recorded as late
- **WHEN** a cancellation is folded before the destruction happens
- **THEN** the note is not destroyed
- **AND** nothing records a late cancellation

#### Scenario: An uncancelled destruction records nothing
- **WHEN** a redaction is carried out and never cancelled
- **THEN** nothing records a late cancellation
