## ADDED Requirements

### Requirement: A day means the day the reader is in

Where a surface shows what happens on a day, it SHALL take that day to begin and
end in the reading zone, and every surface SHALL agree about which day a given
moment falls on.

A day is not a fixed span of instants. "2026-10-05" begins at a different moment
in Denver than in Berlin, and a surface that builds its window in a zone the
reader is not in answers a question nobody asked — quietly, and worst in the
evening, which is when somebody is most likely to be looking at an agenda.

Two surfaces reading one store SHALL NOT disagree about what day something is
on. That disagreement is worse than either answer alone: it makes the archive
look unreliable rather than the surface look wrong.

The same zone SHALL decide whether the day being shown is today, because that is
what governs whether work already overdue is shown at all, and the overdue
boundary is defined against the reading zone.

#### Scenario: An evening appointment is on the evening's day
- **WHEN** an appointment is at 19:00 in the reading zone, west of UTC
- **THEN** it SHALL appear on that day's agenda
- **AND** SHALL NOT appear on the next day's

#### Scenario: An early appointment east of UTC is on its own day
- **WHEN** an appointment is early in the morning in a zone ahead of UTC
- **THEN** it SHALL appear on that day's agenda
- **AND** SHALL NOT appear on the previous day's

#### Scenario: Two surfaces agree
- **WHEN** the same day is asked for in the window and in the terminal
- **THEN** both SHALL answer with the same entries

#### Scenario: Today is today in the reading zone
- **WHEN** the day being shown is today in the reading zone
- **THEN** work already overdue SHALL be shown with it
