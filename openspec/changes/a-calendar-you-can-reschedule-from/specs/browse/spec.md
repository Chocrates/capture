## MODIFIED Requirements

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

#### Scenario: A cancelled occurrence is not drawn as happening, and can be restored
- **WHEN** an occurrence has been cancelled
- **THEN** it is not drawn as an occurrence and does not mark its day as busy
- **AND** the day's list shows it as skipped, with a way to restore it

## ADDED Requirements

### Requirement: A calendar shows a month or a week

The phone and the desktop SHALL show a calendar as a month grid, and the phone
SHALL also offer a week strip, switched in place. Selecting a day SHALL show
that day's agenda beneath. Moving between months or weeks and returning to
today SHALL be one gesture each. A day with something on it SHALL be marked,
and the marks SHALL come from the same read as the day's agenda.

#### Scenario: Next week's appointments
- **WHEN** a person opens the calendar and moves forward one week
- **THEN** each day with an occurrence, all-day item or dated task is marked
- **AND** selecting a marked day lists them

#### Scenario: Switching shape keeps the day
- **WHEN** a day is selected in the month grid and the week strip is chosen
- **THEN** the week containing that day is shown with it still selected

### Requirement: One occurrence can be changed where it is shown

Selecting an occurrence on the phone or the desktop SHALL offer Done, Move this
one, Skip this one, and Change this and later. Move SHALL take text and SHALL
show the resolved time before it is written. Change this and later SHALL split
the series as the recurrence capability specifies.

#### Scenario: Sick on Monday
- **WHEN** the person selects Monday's gym, chooses Move this one and types "thursday 10am"
- **THEN** the resolved time Thursday 15 October 10:00 is shown before confirming
- **AND** after confirming, Thursday shows the gym at 10:00 labelled as moved from Monday
- **AND** the series' other Mondays are unchanged

#### Scenario: Skipping and restoring
- **WHEN** an occurrence is skipped from the calendar
- **THEN** its day shows it as skipped
- **AND** Restore brings it back at its original time

### Requirement: An unclear hour is asked on the phone

The phone SHALL offer every unclear hour with am and pm choices, in the same
places it offers a near miss, and on the note's page.

#### Scenario: The gym asks once
- **WHEN** "gym every monday at 2" has been captured on the phone
- **THEN** the phone offers "2am or 2pm?" for it
- **AND** choosing pm times the series and the offer goes away
