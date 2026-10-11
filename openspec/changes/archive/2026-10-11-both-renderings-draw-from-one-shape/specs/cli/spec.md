## ADDED Requirements

### Requirement: The JSON carries everything the prose says

For every command that takes `--json`, the JSON output SHALL carry every fact
the prose output can show. It MAY carry more.

Both renderings SHALL be drawn from one value built once per command, and the
prose rendering SHALL have no access to the store, so that a fact cannot reach
the prose without passing through the value the JSON is rendered from.

The JSON output SHALL be valid JSON for every input, including notes whose text
contains quotes, backslashes and control characters.

The JSON path has been the poorer one repeatedly, because it is the one nobody
looks at while writing a command. A script given less than a terminal is a
script that cannot answer the question the command exists for — `due` without
its delivery state is a list of things with the reason they did or did not fire
taken out.

#### Scenario: The agenda's carried-forward work reaches the JSON
- **WHEN** today has an overdue task and an owed occurrence, and the agenda is
  asked for with `--json`
- **THEN** the output SHALL carry both, as it does in prose

#### Scenario: The due list says what it will do
- **WHEN** something is due and `due` is asked for with `--json`
- **THEN** each entry SHALL carry its delivery state — delivered, waiting, now,
  held to an hour, or too late — and the output SHALL carry the unread count

#### Scenario: A note's page in JSON says what it was recognised as
- **WHEN** a note promoted to a task is shown with `--json`
- **THEN** the output SHALL carry the promotion's kind and descriptor

#### Scenario: A lapsed mute is not reported as a mute
- **WHEN** a note's mute has ended and it is listed with `--json`
- **THEN** it SHALL be reported as not muted
- **AND** the output SHALL carry when the mute ended

#### Scenario: Every JSON output parses
- **WHEN** any command is given `--json`, over an archive holding text with
  quotes, backslashes, newlines and control characters
- **THEN** its output SHALL parse as JSON
