## ADDED Requirements

### Requirement: An hour without am or pm is asked, not guessed

After a marker, a bare number from 1 to 12 with no meridiem and no colon SHALL
NOT be read as a time of day. The capture SHALL still promote to its date or
rule without a time, and the recognition SHALL record the unclear hour so a
surface can ask which was meant. A bare 0 or 13–23 after a marker SHALL be read
as a 24-hour time, as before.

#### Scenario: An afternoon appointment is not put at night
- **WHEN** a capture reads "go to the gym every monday at 2"
- **THEN** it promotes to a weekly series on Monday with no time of day
- **AND** an unclear hour of 2 is recorded
- **AND** no occurrence is placed at 02:00

#### Scenario: A one-off with a bare hour keeps its day
- **WHEN** a capture reads "dentist on oct 15th at 2"
- **THEN** it promotes to an all-day event on October 15
- **AND** an unclear hour of 2 is recorded

#### Scenario: A meridiem or a 24-hour hour is not asked about
- **WHEN** a capture reads "gym every monday at 2pm" or "standup every weekday at 14"
- **THEN** the time is 14:00
- **AND** no unclear hour is recorded

### Requirement: An unclear hour is settled by an ordinary edit

Every surface that offers near misses SHALL offer an unclear hour with two
choices, naming what will be inserted. Accepting SHALL insert `am` or `pm`
directly after the number as an ordinary edit, after which the capture
re-promotes with that time. Nothing SHALL be inserted without the person
choosing.

#### Scenario: Choosing pm times the series
- **WHEN** "go to the gym every monday at 2" is settled as pm
- **THEN** its text reads "go to the gym every monday at 2pm"
- **AND** it occurs on Mondays at 14:00
- **AND** no unclear hour is offered for it any more
