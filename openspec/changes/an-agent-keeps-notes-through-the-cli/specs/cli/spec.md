## ADDED Requirements

### Requirement: Write commands report what they wrote as JSON

Every command that writes SHALL accept `--json` and print one object naming the
note, its version and any notice. Under `--json`, a failure SHALL print an
error object on stderr and exit with the typed code.

#### Scenario: A new page reports its name
- **WHEN** `capture page new "Car maintenance" --json` is run
- **THEN** it prints an object with `name` `car-maintenance`, the slug and the version

#### Scenario: A refused write
- **WHEN** an edit is refused for a stale version under `--json`
- **THEN** stderr holds `{"error": "conflict", ...}` and the exit code is the conflict code

### Requirement: An edit reads its text from stdin

`capture edit NAME --stdin` SHALL take the new text from standard input,
keeping its line breaks.

#### Scenario: A multi-line page
- **WHEN** three lines are piped to `capture edit packing --stdin`
- **THEN** the page's text is those three lines

### Requirement: An edit can name the version it replaces

`capture edit NAME --expect VERSION` SHALL be refused, writing nothing, unless
VERSION is the note's current version.

#### Scenario: The person saved first
- **WHEN** a script read version v1, the person saved v2, and the script runs `edit --expect v1`
- **THEN** the edit is refused and the page is still v2

### Requirement: Lines can be appended without overwriting

`capture append NAME` SHALL add its text to the end of the note's current text,
from arguments or stdin, and SHALL NOT lose any version written before it.

#### Scenario: Append races an edit
- **WHEN** the person saves a page and an agent appends a line at the same moment
- **THEN** the resulting text holds both the person's edit and the appended line
