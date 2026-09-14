## ADDED Requirements

### Requirement: List items are expanded into the projection and are rebuildable

The projection SHALL expand the items of a list capture into rows of their own.
Expanded items SHALL be derived data and SHALL be discarded and recomputed by a
rebuild, on the same terms as expanded occurrences.

An item SHALL carry an identity derived from its list capture and its position,
so that the same item is recognisable across rebuilds without being stored in
the log.

#### Scenario: A rebuild reproduces the expansion exactly
- **WHEN** the projection is rebuilt
- **THEN** the expanded items match those the previous generation held
- **AND** each item's identity is unchanged

#### Scenario: Completion survives a rebuild
- **WHEN** an item has been completed
- **AND** the projection is rebuilt
- **THEN** the item is still marked done

#### Scenario: An older capture becomes a list on a rebuild
- **WHEN** a capture written before this change matches the list form
- **AND** the projection is rebuilt
- **THEN** its items are expanded
- **AND** the log is untouched

### Requirement: A list is queryable without reading the text

The projection SHALL answer, for a tag, the items expanded under it and the
tasks carrying it, ordered by their display order and then by capture time,
without reading any note's text.

#### Scenario: Reading a list touches no text
- **WHEN** a list is read
- **THEN** the items and tasks are answered from the projection alone

#### Scenario: Items order before their tag's loose tasks
- **WHEN** a list holds both expanded items and a task tagged by hand
- **THEN** the items are returned in their display order
- **AND** the tasks follow, newest first
