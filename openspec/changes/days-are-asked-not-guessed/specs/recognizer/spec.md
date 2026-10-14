## ADDED Requirements

### Requirement: A weekday beside a date is read with the date

The recognizer SHALL read a weekday adjacent to a complete date, in either
order and with at most a comma between, as one anchor. Where the weekday is the date's weekday,
the recognition SHALL be the date, with any time that follows, at the band the
date alone would have. Where it is not, nothing SHALL be promoted and an unclear
day SHALL be recorded offering the named weekday nearest the date and the date.

#### Scenario: The therapy note
- **WHEN** a capture made on Tuesday 2026-10-13 reads "therapy Thursday 2026-10-22 at 4pm"
- **THEN** it is an event at 2026-10-22 16:00
- **AND** no suggestion is offered

#### Scenario: Spelled with a month name
- **WHEN** a capture reads "therapy thursday oct 22 at 4pm" or "therapy oct 22, thursday 4pm"
- **THEN** it is an event at 22 October 16:00

#### Scenario: They disagree
- **WHEN** a capture reads "therapy Thursday 2026-10-23 at 4pm" (a Friday)
- **THEN** nothing is promoted
- **AND** an unclear day offers 2026-10-22 and 2026-10-23

### Requirement: "next" before a weekday is asked when it could mean either week

"next X" SHALL be asked when the first X after today falls in the current
Monday-to-Sunday week, offering that day and the one seven days later. Otherwise
it SHALL be the first X after today. "last X" SHALL be the most recent X before
today. "this X" SHALL keep counting today.

#### Scenario: Early in the week
- **WHEN** a capture made on Tuesday 2026-10-13 reads "therapy next thursday at 4pm"
- **THEN** nothing is promoted
- **AND** an unclear day offers 2026-10-15 and 2026-10-22

#### Scenario: Late in the week
- **WHEN** a capture made on Friday 2026-10-16 reads "therapy next thursday at 4pm"
- **THEN** it is an event at 2026-10-22 16:00

#### Scenario: On the day itself
- **WHEN** a capture made on Thursday 2026-10-15 reads "next thursday"
- **THEN** it is 2026-10-22, not today
