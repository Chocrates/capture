## ADDED Requirements

### Requirement: An agent writes as its own origin

An agent SHALL be enrolled as a separate origin with role `agent`. Writes made
on its behalf SHALL be signed by that origin, and its history SHALL name it.

#### Scenario: An agent's edit is attributed
- **WHEN** `capture agent add claude` is run and `CAPTURE_AS=claude capture page new "Build notes"` follows
- **THEN** `capture log build-notes` names `claude` as the origin of every event it wrote

#### Scenario: An agent is cut off
- **WHEN** the agent `claude` is retired
- **THEN** further writes as `claude` are refused
- **AND** events it wrote before retirement remain

### Requirement: What an agent writes is not put in front of the person

Notes whose first capture came from an agent origin SHALL NOT appear in the
timeline, review, settle or recent surfaces. They SHALL appear in search, page
lists, links and entity pages, labelled with the agent's name.

#### Scenario: Fifty pages, no backlog
- **WHEN** an agent creates fifty pages
- **THEN** the timeline and review show none of them
- **AND** a search finds them, each saying which agent wrote it

#### Scenario: An agent edits the person's page
- **WHEN** an agent appends a line to a page the person wrote
- **THEN** the page shows the new line
- **AND** its history attributes that version to the agent
- **AND** the page does not move in any list

### Requirement: An agent cannot store a credential

A write from an agent origin whose text looks like it holds a credential SHALL
be refused and nothing SHALL be written.

#### Scenario: A token in agent text
- **WHEN** an agent tries to append "API key: sk-abc123def456ghi789jkl012"
- **THEN** the write is refused with a reason
- **AND** the page is unchanged

### Requirement: The harness setup is printed, not installed

`capture agent instructions` SHALL print a skill describing the read-then-write
loop and a permission allowlist that excludes destructive commands. It SHALL
NOT write to any harness configuration.

#### Scenario: Printing the instructions
- **WHEN** `capture agent instructions --name claude` is run
- **THEN** a skill and an allowlist are printed
- **AND** the allowlist admits `capture search` and `CAPTURE_AS=claude capture edit`
- **AND** it does not admit `capture redact`, `forget`, `retire`, `pair` or `set`
- **AND** no file outside the output is written
