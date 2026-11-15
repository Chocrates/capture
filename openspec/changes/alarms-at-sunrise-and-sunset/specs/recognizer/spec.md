## ADDED Requirements

### Requirement: Sun phrases are read as times

The recognizer SHALL read "at sunrise", "at sunset", "N minutes/hours
before/after sunrise/sunset", and those phrases after a recurrence head or a day
word as sun-anchored times. An offset beyond six hours SHALL be asked about, not
read.

#### Scenario: Every day at sunrise
- **WHEN** "wake up every day at sunrise" is captured
- **THEN** it is read as a daily series at sunrise

#### Scenario: Before sunset tomorrow
- **WHEN** "tomorrow 30 min before sunset walk the dog" is captured
- **THEN** it is read as one moment, tomorrow, 30 minutes before sunset

#### Scenario: Weekdays
- **WHEN** "weekdays at sunrise run" is captured
- **THEN** it is read as a Monday–Friday series at sunrise

### Requirement: The sun as a subject is not a time

A sun word without "at", "before" or "after" SHALL NOT be read as a time.
`>sunset` SHALL NOT be read as Sunday.

#### Scenario: A remark
- **WHEN** "watched the sunset from the pier" is captured
- **THEN** it stays a note with no time
