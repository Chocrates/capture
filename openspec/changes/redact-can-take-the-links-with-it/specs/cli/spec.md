## ADDED Requirements

### Requirement: A redaction can take the notes linking to its target

The command line SHALL offer a way to redact a note together with every note
that links to it.

A note's slug is derived from its opening words, and a note linking to it
carries that slug inside its own verbatim text. Destroying the target
re-derives its name from its id, which stops the name carrying the words — and
leaves the copies already written into other notes, where no redaction reaches
them.

It SHALL follow links one hop only. A note linking to a note linking to the
target has copied nothing of the target, and a transitive sweep turns a
deliberate act into an accident.

#### Scenario: Redacting a note and what points at it
- **WHEN** a note is redacted with its links
- **THEN** the target is destroyed
- **AND** every note linking to it is destroyed

#### Scenario: Links are followed one hop
- **WHEN** a note links to a note that links to the target
- **THEN** that further note is not destroyed

#### Scenario: Without the flag nothing changes
- **WHEN** a note is redacted without asking for its links
- **THEN** only that note is destroyed

### Requirement: The confirmation names every note that will be destroyed

The confirmation SHALL list each note the redaction will destroy, by name.

§1.1 makes this the one confirmation in the system that must not be trained
away by appearing routinely. A flag that quietly widens what a familiar
confirmation covers is precisely how it would be — somebody who has typed the
name once learns the prompt means one note.

#### Scenario: Several notes will go
- **WHEN** a redaction with links will destroy more than the target
- **THEN** the confirmation names each of them

#### Scenario: Declining destroys nothing
- **WHEN** the confirmation is declined
- **THEN** no note is queued for destruction

### Requirement: Too many to name is refused

The command SHALL refuse, and say so, where the target and the notes linking to
it exceed the number a redaction may name — rather than redacting those that fit.

Redacting the first that fit would leave the rest holding the words the whole
operation exists to remove, while reporting success.

#### Scenario: More linking notes than a redaction may name
- **WHEN** the target and its backlinks exceed the cap
- **THEN** nothing is redacted
- **AND** the refusal says how many there were
