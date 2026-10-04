## ADDED Requirements

### Requirement: Mentions are derived from the note's current text

The projection SHALL hold the mentions of each note, derived from the text the
note has now, and SHALL recompute them when the text changes.

Unlike a tag, a mention SHALL NOT be added or retracted by an event of its own: a
tag is something done to a note, and a mention is part of what the note says.

A note with no readable text SHALL name nobody. That is what keeps a redacted
note, and one with a redaction queued, off every entity page — the mentions go
when the text does, rather than having to be found and removed.

#### Scenario: Editing the text renames who is mentioned
- **WHEN** a note's text is edited to name a different person
- **THEN** it SHALL appear on the new person's page
- **AND** SHALL NOT appear on the previous one's

#### Scenario: A redacted note names nobody
- **WHEN** a note is redacted
- **THEN** it SHALL contribute no mentions

### Requirement: Entity identity is projected, and rebuildable

The projection SHALL hold entities and the names that resolve to them, and both
SHALL be reconstructible by folding the log alone.

`ent_id` SHALL be derived from the first spelling seen for an entity, under a
rule that depends on the event set and nothing else — never on the wall clock and
never on which device folded. Two devices folding one log SHALL derive the same
ids.

#### Scenario: A rebuild reproduces the same entities
- **WHEN** the projection is rebuilt
- **THEN** every `ent_id` is the same as the previous generation held
- **AND** every name resolves to the same entity

#### Scenario: Two devices agree
- **WHEN** two devices hold the same events and fold independently
- **THEN** they derive the same `ent_id` for each entity

#### Scenario: A late arrival does not fork an entity
- **WHEN** a capture naming an entity arrives after later events already did
- **THEN** it SHALL join the existing entity rather than creating a second
