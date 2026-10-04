# projection Specification

## Purpose
TBD - created by archiving change m2-types-and-review. Update Purpose after archive.
## Requirements
### Requirement: Occurrences are expanded into the projection and are rebuildable

The projection SHALL expand recurring occurrences over a rolling horizon,
extended in the background and on view access. Expanded occurrences SHALL be
derived data and SHALL be discarded and recomputed by a rebuild.

#### Scenario: A rebuild reproduces the expansion exactly
- **WHEN** the projection is rebuilt
- **THEN** the expanded occurrences match those the previous generation held

#### Scenario: A timezone database update needs no migration
- **WHEN** the system timezone database changes
- **THEN** the correct expansion is obtained by re-folding
- **AND** the log and the stored instants are untouched

### Requirement: The due queue carries review and intent deadlines

The due-work queue SHALL carry scheduled review sessions and time-based intent
deadlines alongside redaction deadlines, and SHALL be processed by the same
mechanism: every process startup is the floor, with a resident window making a
deadline land near its time.

#### Scenario: A deadline that fell while nothing ran is handled at startup
- **WHEN** a review or intent deadline elapsed while no process was running
- **THEN** it is processed at the next startup
- **AND** nothing is lost because no timer was running

### Requirement: Promoted fields are queryable without reading the text

The projection SHALL store the structured fields of a promoted note and its
descriptor so that agenda, calendar and task queries do not parse text at read
time.

#### Scenario: The agenda is a query
- **WHEN** the agenda for a day is requested
- **THEN** it is answered from stored fields and expanded occurrences

### Requirement: The rendering a surface draws comes from the projection

The rendering of a promotion SHALL be supplied by the projection rather than
composed by each surface. A surface SHALL NOT format fields into a description
of its own.

Two surfaces composing the same fields independently would be two renderings
that can disagree, and the disagreement would be invisible — each looks correct
on its own screen. This is the same argument that puts the descriptor in the
projection rather than computing it per view.

#### Scenario: One rendering, every surface
- **WHEN** a promoted note is drawn on two surfaces
- **THEN** both show the same rendering
- **AND** neither composed it from the fields itself

#### Scenario: The rendering is available without reading the text
- **WHEN** a surface asks for a note
- **THEN** the promotion's kind and its rendering are answered with it

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

### Requirement: Items are projected from the capture's current text

The projection SHALL hold the items of each capture, derived from the text it
has now, in written order, and SHALL recompute them when the text changes.

A capture with no readable text SHALL have no items, which is what keeps a
destroyed capture's list off every surface — the same rule the mentions of a
redacted note follow, and for the same reason.

#### Scenario: Editing the text re-expands the items
- **WHEN** a capture's items are edited
- **THEN** the projected items SHALL match what the capture now says

#### Scenario: A destroyed capture has no items
- **WHEN** a capture that wrote items is redacted
- **THEN** it SHALL contribute no items to any surface

### Requirement: Unread captures are countable from a read-only handle

The projection SHALL answer how many captures have not been read by the current
grammar, from a handle opened read-only and without involving the writer. It is
already able to *find* them — that is what drives the reading pass — and the
same index answers the count.

This is required read-only specifically because it is what lets a surface be
honest while the writer is unavailable. A staleness report that needs the writer
in order to say the writer has not run is worthless at the moment it is needed
(§8.8).

No new table. The captures needing reading are identified by comparing a stored
grammar version against the current one, which the projection already indexes.

#### Scenario: Counted without the writer
- **WHEN** the count is asked for on a read-only handle
- **THEN** it is answered

#### Scenario: Counted while the writer lock is held
- **WHEN** another process holds the writer lock
- **THEN** the count is still answered

#### Scenario: A grammar bump makes every capture outstanding
- **WHEN** the grammar version increases
- **THEN** the count includes the captures read by the older grammar

#### Scenario: Zero when everything has been read
- **WHEN** every capture has been read by the current grammar
- **THEN** the count is zero

### Requirement: A superseded generation does not accumulate

The store SHALL remove projection generations it has superseded. A rebuild
writes a new generation beside the current one and moves the pointer, which
leaves the old file intact deliberately — readers keep the file they already
opened — but it SHALL NOT be left there for ever.

The current generation and the one immediately before it SHALL be kept.
Anything older SHALL be removed. Keeping one spare is what makes the gap
between a reader resolving the pointer and opening the file safe, without
requiring a read to take a lock (§8.8).

Removal SHALL happen while the writer lock is held, and a removal that fails
SHALL NOT fail the operation it happened during. Disk that could not be
reclaimed is disk to reclaim next time, not a reason to refuse the store.

#### Scenario: Older generations are removed
- **WHEN** a store has been rebuilt more than once
- **AND** a session is opened
- **THEN** generations older than the one before the current one no longer exist

#### Scenario: The current generation and one spare survive
- **WHEN** generations are removed
- **THEN** the current generation still exists
- **AND** the generation immediately before it still exists

#### Scenario: A rebuild does not strand the generation it replaced
- **WHEN** a projection is rebuilt
- **THEN** the generation it replaced is still readable by anything that had resolved it

#### Scenario: Removal that fails is not fatal
- **WHEN** a superseded generation cannot be removed
- **THEN** the session still opens

### Requirement: A rebuilt projection reports what the log has set aside

A rebuild SHALL repopulate the record of set-aside events from the log, which
holds the authoritative mark. The projection is derived and disposable (§1.3),
so anything it is the only copy of is lost whenever it is rebuilt — and the set
of events this device is refusing to use is not something a surface may be
wrong about.

#### Scenario: Set-aside events survive a rebuild
- **WHEN** events have been set aside
- **AND** the projection is rebuilt
- **THEN** they are still reported as set aside
- **AND** the reason recorded for each is unchanged

#### Scenario: A reissued retirement still re-admits after a rebuild
- **WHEN** events are held because a retirement excluded them
- **AND** the projection is rebuilt
- **AND** the retirement is reissued with a later head
- **THEN** the events below that head are re-admitted

#### Scenario: Nothing set aside stays nothing
- **WHEN** no event has been set aside
- **AND** the projection is rebuilt
- **THEN** nothing is reported as set aside

### Requirement: A rebuilt record is ordered by when the event was authored

Where a set-aside record is restored by a rebuild, the moment it carries SHALL
be the event's own timestamp. When this device first saw the event is not in the
log — it existed only in the projection being replaced — and the authored
instant is stable across rebuilds, so the report does not reorder itself each
time one happens.

#### Scenario: The order does not change between rebuilds
- **WHEN** a projection holding set-aside records is rebuilt twice
- **THEN** the records are reported in the same order both times

