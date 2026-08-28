## ADDED Requirements

### Requirement: A review session is bounded and ends

A review session SHALL offer a fixed small number of items and SHALL then end
and say so. An unbounded queue SHALL NOT be presented.

#### Scenario: The session finishes
- **WHEN** the items in a session have been offered
- **THEN** the session states that it is over
- **AND** no further items are offered until the next scheduled session

### Requirement: Selection follows a stated priority order

Items SHALL be selected in the order: notes carrying a forward marker the
recognizer found; notes attached to an active project or upcoming calendar
event; notes never engaged with since capture, oldest first; notes whose ambient
context matches the current context.

#### Scenario: A forward marker outranks age
- **WHEN** one note says "revisit Q2" and another is merely old
- **THEN** the note carrying the forward marker is offered first

### Requirement: The outcome is shown, engaged or deferred, and nothing is scored

A `review` event SHALL record only whether the item was shown, engaged with, or
deferred. No score, streak, accuracy, grade or history of failure SHALL be
recorded at any aggregation, anywhere.

#### Scenario: No memory judgement is recorded
- **WHEN** a review item is offered and the user does nothing with it
- **THEN** the recorded outcome is that it was shown
- **AND** nothing records whether anything was recalled

#### Scenario: There is no streak to break
- **WHEN** review sessions are missed for a month
- **THEN** no aggregate, streak or accuracy figure exists to display

### Requirement: Scheduling is tuned by engagement, not accuracy

Intervals SHALL expand based on engagement. An item engaged with SHALL recur
less often. An item deferred twice SHALL stop being offered rather than being
pressed.

#### Scenario: Deferring twice stops the offering
- **WHEN** an item is deferred on two consecutive occasions
- **THEN** it is no longer offered

### Requirement: Muting and pausing always work, without a reason

`mute` SHALL apply to review. "Pause review" and "less often" SHALL always be
available and SHALL NOT require a reason.

#### Scenario: A muted note stops resurfacing
- **WHEN** a note is muted
- **THEN** review does not offer it
- **AND** the note is not destroyed and the mute is reversible
