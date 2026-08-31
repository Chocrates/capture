# browse Specification

## Purpose
TBD - created by archiving change m2-types-and-review. Update Purpose after archive.
## Requirements
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


### Requirement: Every mutation is reachable from the window

The browse surface SHALL offer every mutation the command line offers against a
note, and SHALL offer device management where the device is already shown.

§8.4 makes the window the first-party surface, and §8.10 scoped it to "every
mutation" for exactly this reason: muting exists so that suppressing something
is *cheaper* than destroying it, and that gradient inverts the moment
suppression needs a shell while destruction is a button. The same argument
applies to a lost device — the moment somebody needs to retire one is not a
moment to be looking up a command.

Splitting a series SHALL be offered only on a series, since it is the only
thing a rule can be split at.

#### Scenario: Changing a series from the window
- **WHEN** a note is a series
- **THEN** the page SHALL offer to change it from a date onward
- **AND** SHALL NOT offer it on a note that is not a series

#### Scenario: Retiring a device from the window
- **WHEN** a device other than this one is listed
- **THEN** it SHALL be possible to retire it
- **AND** this SHALL require the device's key to be typed, as destruction
  requires the note's name

### Requirement: A shared confirmation says what it is confirming

Where one dialog serves more than one irreversible action, it SHALL state the
action it is confirming rather than the one it was written for.

This is not presentation. The dialog was written for destruction and says so in
the markup; reused unchanged it asked "Destroy this content?" over a device
retirement, which deletes nothing. Confirmation is the one screen whose entire
job is being precise about what is about to happen.

#### Scenario: Confirming something that is not a destruction
- **WHEN** the confirmation is raised for an action that destroys nothing
- **THEN** its title, explanation and accept button SHALL describe that action
