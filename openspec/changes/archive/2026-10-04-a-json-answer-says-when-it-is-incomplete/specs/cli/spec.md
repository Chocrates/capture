## ADDED Requirements

### Requirement: The incompleteness report reaches the JSON output too

Where a command reports that captures are unread, it SHALL do so in its `--json`
output as well as in its prose, and a consumer SHALL be able to tell an empty
answer from an unread one.

The requirement that these commands report incompleteness was read as satisfied
because the prose path did it. `tasks --json` carried the count; `agenda --json`
answered a bare array and returned before the reporting ran at all. So the same
question, asked two ways, was hedged in a terminal and confident in a script.

A script is the consumer least able to notice. A person reading "nothing on
Tuesday" in a terminal sees the line underneath it; a program receiving `[]`
sees an empty day. The two differ most when the backlog is large, which is
exactly when the sweep is most likely to be behind.

Reporting SHALL remain alongside the answer rather than in place of it. A read
that failed because the sweep was behind would be §8.8's failure with the sign
flipped.

#### Scenario: An agenda asked for as JSON says what it has not read
- **WHEN** captures are unread and the agenda is asked for with `--json`
- **THEN** the output SHALL carry how many are unread
- **AND** SHALL still carry the day's entries

#### Scenario: An empty day is distinguishable from an unread one
- **WHEN** a day has no entries and captures are unread
- **THEN** a consumer SHALL be able to tell that from a day that is genuinely empty

#### Scenario: Nothing unread says nothing of note
- **WHEN** every capture has been read
- **THEN** the output SHALL report zero unread rather than omitting the field
