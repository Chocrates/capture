## ADDED Requirements

### Requirement: An entity exists because a note named it

An entity SHALL exist as soon as any note's text names it, with no event written
and no curation performed. Its page SHALL be readable immediately and SHALL be
empty of everything except the notes that name it.

§3.7 makes entities one of four organising axes that populate without the user,
and that is the whole claim: an axis requiring a record to be created first is a
filing ritual with extra steps, and §0 rates those as the thing most likely not
to happen.

#### Scenario: A name reaches a page with nothing created
- **WHEN** a capture names `@sarah` and nothing else has ever mentioned her
- **THEN** an entity page for `@sarah` SHALL be readable
- **AND** no event beyond the capture SHALL have been written

#### Scenario: A name nobody has written
- **WHEN** a page is asked for a name no note mentions
- **THEN** the surface SHALL say that nothing names it
- **AND** SHALL NOT report this as an error or a missing record

### Requirement: An entity's identity survives its name changing

An entity SHALL carry an `ent_id` derived from the first spelling under which it
was seen, and that id SHALL NOT be recomputed when the entity is renamed or when
further spellings are attached to it.

§4.3 gives the reason and it is a list of failures, not a preference: a bare name
means renaming a person silently forks their page, two people called Sarah cannot
be told apart, and there is no merge path when extraction produces both "sarah"
and "sarah chen".

Identity SHALL fold case; display SHALL NOT. `@Sarah` and `@sarah` are one
entity, whose displayed name is the spelling first written. Two people who really
are different SHALL be distinguishable by being two entities, not by being two
spellings.

#### Scenario: Renaming does not fork the page
- **WHEN** an entity is renamed
- **THEN** its `ent_id` SHALL be unchanged
- **AND** every note that mentioned it under the old spelling SHALL still be on
  its page

#### Scenario: Case is not two people
- **WHEN** one note names `@Sarah` and another names `@sarah`
- **THEN** both SHALL be on one entity's page
- **AND** the displayed name SHALL be the spelling written first

#### Scenario: Two people, one name
- **WHEN** two entities exist with the same display name
- **THEN** they SHALL remain distinct
- **AND** neither SHALL be merged into the other without being asked

### Requirement: Several names may reach one entity

A name SHALL resolve to an entity through a stored mapping rather than by string
equality, so that more than one spelling can reach one entity.

Extraction cannot know that two spellings are one person, and §3.6's lesson about
`carries_forward` applies directly: two spellings are indistinguishable from two
people by phrasing alone, so the system SHALL NOT guess. A new spelling therefore
SHALL create a new entity, and joining them SHALL be a person's gesture.

#### Scenario: A new spelling is a new entity until somebody says otherwise
- **WHEN** a note names a spelling never seen before
- **THEN** a new entity SHALL exist for it
- **AND** nothing SHALL be merged automatically

#### Scenario: Both spellings reach one page after a merge
- **WHEN** one entity has been merged into another
- **THEN** either spelling SHALL resolve to the surviving entity
- **AND** the surviving page SHALL hold the notes that named either

### Requirement: Merging follows a pointer and loses nothing

Merging SHALL record that one entity is now the other, and resolution SHALL
follow that pointer. Merging SHALL NOT rewrite, move or delete any note, mention
or capture.

Nothing in an append-only log can take a merge back quietly, so a merge SHALL be
expressible as an ordinary event whose effect is a redirection rather than a
rewrite. A merge chain SHALL resolve to its end, and a cycle SHALL NOT make an
entity unreachable.

#### Scenario: A merge moves no data
- **WHEN** two entities are merged
- **THEN** no capture text SHALL change
- **AND** no mention row SHALL be removed

#### Scenario: A chain resolves to its end
- **WHEN** A has been merged into B and B into C
- **THEN** a name reaching A SHALL resolve to C

#### Scenario: A cycle does not lose an entity
- **WHEN** merge events would form a cycle
- **THEN** resolution SHALL still terminate
- **AND** the entity SHALL remain reachable rather than becoming unaddressable

### Requirement: An entity page may be backed by a note

An entity SHALL be allowed a backing note — the note that *is* the project,
rather than one that mentions it — and where one is bound, the entity page SHALL
show it above the filtered timeline.

Binding SHALL set the entity's own `note_tgt` and SHALL NOT change what the note
is. §4.3 is explicit that this does not add a value to the note's type, which
enumerates `note | task | event | series` and does not include an entity — a
backing note stays whatever it already was, and may be a task or an event.

Most entities SHALL have none, and that SHALL NOT be presented as incomplete.

#### Scenario: Binding does not retype the note
- **WHEN** a task is bound as an entity's backing note
- **THEN** it SHALL still be a task
- **AND** it SHALL still appear wherever tasks appear

#### Scenario: No backing note is the ordinary case
- **WHEN** an entity has no backing note
- **THEN** its page SHALL show the filtered timeline alone
- **AND** SHALL NOT prompt for one

#### Scenario: A backing note still mentions what it mentions
- **WHEN** a backing note names other entities
- **THEN** it SHALL appear on their pages too

### Requirement: An entity page is a filtered timeline

An entity page SHALL be the notes naming that entity, newest first, and SHALL NOT
be a separate store of content.

A muted note SHALL NOT appear while its mute holds, and a redacted note SHALL
name nobody — a destroyed note's mentions SHALL be absent from every entity page,
because the mentions are derived from text the note no longer has.

#### Scenario: Ordered by when it was captured
- **WHEN** an entity page is read
- **THEN** the notes naming it SHALL be newest first

#### Scenario: A destroyed note names nobody
- **WHEN** a note that named an entity is redacted
- **THEN** it SHALL NOT appear on that entity's page

#### Scenario: A muted note is out of the way
- **WHEN** a note naming an entity is muted
- **THEN** it SHALL NOT appear on that page while the mute holds

### Requirement: An entity can be muted

Muting SHALL accept an entity as a scope, removing every note naming it from
every surfacing path until the mute lapses.

`m1` required this and deferred it for want of entities to scope to. §2.6's
gradient is the reason it matters: suppressing something must stay cheaper than
destroying it, and an entity somebody does not want to be reminded of is exactly
the case where the only alternative on offer would otherwise be redaction.

#### Scenario: Muting an entity quietens its notes
- **WHEN** an entity is muted
- **THEN** notes naming it SHALL NOT surface while the mute holds
- **AND** they SHALL NOT be altered or destroyed

#### Scenario: A mute lapses
- **WHEN** an entity's mute reaches its end
- **THEN** its notes SHALL surface again without anything being restored
