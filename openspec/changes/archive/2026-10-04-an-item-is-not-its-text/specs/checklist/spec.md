## ADDED Requirements

### Requirement: A checklist belongs to the capture that wrote it

The items written in a capture SHALL belong to that capture, in the order they
were written, and SHALL be reachable from it without naming a tag.

A checklist is bounded, ordered and owned. That is what distinguishes it from a
collection, which is everything carrying a tag — unbounded, a query, owned by
nobody. Both are useful and they are not the same thing; addressing a checklist
by tag is what makes one capture's items indistinguishable from another's.

A capture SHALL have at most one checklist. Two lists are two captures.

#### Scenario: Items are the capture's own
- **WHEN** a capture writes items
- **THEN** they SHALL be readable from that capture
- **AND** reading them SHALL NOT require knowing a tag

#### Scenario: Two captures under one tag are two checklists
- **WHEN** two captures each write items under the same tag
- **THEN** each SHALL have its own checklist
- **AND** neither SHALL show the other's items as its own

### Requirement: An item is identified by its place, not by its words

An item SHALL be identified by its position in the capture that wrote it.
Completion SHALL NOT be matched by the text of the item.

Text as identity is what makes a completion fragile in the one way nobody
expects: correcting a word discards what was already done. It also makes two
items reading the same impossible to tell apart, and "milk, milk" is an ordinary
thing to write when two shops are meant.

#### Scenario: Rewording an item keeps it ticked
- **WHEN** an item that has been ticked is reworded by editing the capture
- **THEN** it SHALL remain ticked

#### Scenario: Two items reading the same are two items
- **WHEN** a capture writes the same words twice as two items
- **THEN** each SHALL be tickable independently
- **AND** ticking one SHALL NOT tick the other

#### Scenario: Removing an item takes its completion with it
- **WHEN** an item is removed by editing the capture
- **THEN** its completion SHALL NOT apply to any other item

#### Scenario: Adding an item arrives unticked
- **WHEN** an item is added by editing the capture
- **THEN** it SHALL be open
- **AND** the completions of the items already there SHALL be unchanged

### Requirement: A checklist does not contain the capture that wrote it

A collection gathered by tag SHALL NOT include the capture whose own items it is
showing.

Reachable today because a list capture may also be promoted to a task, and a
collection gathers tasks carrying the tag alongside items of captures carrying
it. The capture then appears beside its own contents and is counted with them,
so a three-item list reports four things outstanding.

#### Scenario: A list capture that is also a task
- **WHEN** a capture that wrote items is also a task
- **THEN** the collection under that tag SHALL show its items
- **AND** SHALL NOT show the capture itself as one of them

### Requirement: Items do not require a tag

A capture SHALL be able to write items without naming a tag.

A task with parts does not want a label. Requiring one means every checklist is
also a collection, which is how the two came to be the same word.

#### Scenario: A checklist with no tag
- **WHEN** a capture writes items without a tag
- **THEN** they SHALL be its items
- **AND** no tag SHALL be recorded for them

#### Scenario: A tagged checklist still carries its tag
- **WHEN** a capture writes items under a tag
- **THEN** the tag SHALL still gather them into its collection
