## ADDED Requirements

### Requirement: The calendar is readable from a terminal

`capture cal` SHALL print a month, defaulting to the current one, and `capture
cal --week [DATE]` SHALL print a week, each day listing what occurs on it from
the same read every other surface uses. Both SHALL take `--json`.

#### Scenario: A month with a weekly series
- **WHEN** a weekly Monday 14:00 series exists and `capture cal 2026-10` is run
- **THEN** every Monday in October 2026 lists it at 14:00

### Requirement: An occurrence is named by its date

An occurrence SHALL be nameable by its original local date in `capture move`,
`capture skip`, `capture unskip`, `capture done` and `capture undone`. A date
matching no occurrence SHALL be refused, naming the nearest ones. Epoch milliseconds
SHALL remain accepted. `capture move` SHALL take `--to TEXT`, read as in the
recurrence capability, and SHALL print the resolved new time.

#### Scenario: Moving this week's gym
- **WHEN** `capture move gym 2026-10-12 --to "thursday 10am"` is run
- **THEN** the 2026-10-12 occurrence is moved to Thursday 2026-10-15 at 10:00
- **AND** the output names both times

#### Scenario: A date the series does not occur on
- **WHEN** `capture skip gym 2026-10-13` is run for a Monday series
- **THEN** it is refused, naming the nearest occurrences

### Requirement: An unclear hour can be settled from the command line

`capture hour NAME am|pm` SHALL settle an unclear hour as an ordinary edit and
print the new text.

#### Scenario: Settling the gym
- **WHEN** `capture hour gym pm` is run on "go to the gym every monday at 2"
- **THEN** the text becomes "go to the gym every monday at 2pm"
