## ADDED Requirements

### Requirement: People and projects are browsable, on every surface

Every first-party surface SHALL offer an index of the people and projects the
archive knows, each reachable as an entity page, and SHALL show how many notes
name each.

The phone is included, and its absence there today is the gap this closes. §8.4
makes reachability the rule and §3.7 makes entities one of four axes that
populate without the user — an axis that populates by itself and is then only
readable on a laptop is one that does not pay for itself on the device where
capture happens.

An index entry SHALL open a page of its own rather than a note, because an entity
is not a note. Where an entity has a backing note, the page SHALL show it above
the timeline.

#### Scenario: A person opens their own page
- **WHEN** a person is chosen from the index
- **THEN** an entity page SHALL open
- **AND** it SHALL NOT open a note instead

#### Scenario: The phone can browse people
- **WHEN** the archive holds notes naming people
- **THEN** the phone SHALL offer them as an index
- **AND** each SHALL lead to the notes naming that person

#### Scenario: Nothing named yet
- **WHEN** no note names anybody
- **THEN** the index SHALL say so plainly
- **AND** SHALL NOT present itself as broken or unconfigured

### Requirement: A mention in a note is a way to the page

Where a surface renders note text, a mention in it SHALL be a way to that
entity's page.

A mention is already the link §4.3 calls the one that carries the load — it
exists without anyone having linked anything. Rendering it as inert text on one
surface and as a link on another makes the same note mean different things
depending on where it is read.

#### Scenario: Following a mention
- **WHEN** note text containing `@sarah` is shown
- **THEN** that mention SHALL lead to her entity page

#### Scenario: A mention that was renamed still leads somewhere
- **WHEN** the entity a mention names has been renamed
- **THEN** the mention SHALL still lead to that entity's page

### Requirement: Renaming, merging and binding are reachable where the page is

An entity page SHALL offer to rename the entity, to merge it into another, and to
bind or unbind a backing note.

These exist because extraction cannot get identity right on its own, and a
correction that needs a different machine than the one that found the problem is
the gradient §8.4 refuses. The phone is where captures are made, so it is where a
duplicated person is first noticed.

Merging SHALL name what it will join and SHALL be confirmed, because it is the
one operation here that makes two pages into one.

#### Scenario: Renaming from the page
- **WHEN** an entity is renamed from its page
- **THEN** the new name SHALL be displayed
- **AND** its notes SHALL be unchanged

#### Scenario: Merging says what it is joining
- **WHEN** a merge is offered
- **THEN** it SHALL name both entities before it is carried out

#### Scenario: Binding a backing note
- **WHEN** a note is bound as the backing note
- **THEN** it SHALL show above the timeline on that page
- **AND** unbinding SHALL be available on the same terms
