## ADDED Requirements

### Requirement: Three sigils name something with a page

`@`, `^` and `#` SHALL produce an entity mention. `>`, `!` and `~` SHALL NOT.

The division is what the sigil is *about*. A person, a project and a tag are
things a note can be *about*, and a page of everything naming one is meaningful.
A date, a priority and a duration are properties of the note itself — a page of
"everything due Friday" is the agenda, and a page of "everything urgent" is a
query over a field that is not even stored.

A sigil with nothing after it SHALL produce no mention, so that a bare `#` and a
`#?` name nothing rather than naming the empty string.

An address SHALL NOT be read as a mention. A sigil SHALL open a token to count,
which is what keeps `sarah@example.com` from naming a person called
`example.com`.

#### Scenario: A person, a project and a tag
- **WHEN** a capture reads `ask @sarah about ^migration #work`
- **THEN** three mentions SHALL be recorded

#### Scenario: A date is not an entity
- **WHEN** a capture carries `>friday`, `!p1` or `~30m`
- **THEN** no mention SHALL be recorded for it

#### Scenario: A bare sigil names nothing
- **WHEN** a capture reads `what is this #?`
- **THEN** no mention SHALL be recorded

#### Scenario: An email address names nobody
- **WHEN** a capture contains `sarah@example.com`
- **THEN** no mention SHALL be recorded
