## ADDED Requirements

### Requirement: A capture opening with a tag sigil and a colon is a list

A capture whose text begins with a tag sigil, a slug and a colon SHALL be
recognised as a list naming that tag, and the text after the colon SHALL be read
as a separated sequence of items. The form SHALL be anchored at the start of the
capture.

Recognition SHALL be tier 1: it is an explicit marker, so it SHALL NOT be scored
and SHALL NOT be overridden by any inference in the same line.

#### Scenario: A list capture names its tag and its items
- **WHEN** a capture reads `#groceries: milk, eggs, bread`
- **THEN** a list carrying the tag `groceries` is recognised
- **AND** it names the items `milk`, `eggs` and `bread` in that order

#### Scenario: A tag elsewhere in the line is not a list
- **WHEN** a capture reads `ask sarah about #groceries: she has the list`
- **THEN** no list is recognised
- **AND** the capture carries the tag `groceries` as an ordinary tag

#### Scenario: A colon without a sigil is prose
- **WHEN** a capture reads `groceries: milk, eggs, bread`
- **THEN** no list is recognised
- **AND** the capture remains an ordinary note

#### Scenario: An empty item list is not a list
- **WHEN** a capture reads `#groceries:` with nothing after the colon
- **THEN** no list is recognised
- **AND** the capture carries the tag `groceries` as an ordinary tag

### Requirement: A list capture writes one event

A list capture SHALL append exactly one event. Items SHALL NOT be written as
separate captures, and the captured text SHALL be stored exactly as given.

#### Scenario: Three items, one event
- **WHEN** a capture naming three items is committed
- **THEN** one event is appended to the log
- **AND** the stored text is the line as typed

#### Scenario: The text is not rewritten
- **WHEN** a list is recognised
- **THEN** the capture's text still reads as it was typed, sigil and colon included

### Requirement: Items are separated by commas and keep their written order

Items SHALL be separated by commas, SHALL be trimmed of surrounding whitespace,
and SHALL carry the order in which they were written as their display order. An
empty item between two separators SHALL be discarded rather than named.

#### Scenario: Spacing around a separator does not matter
- **WHEN** a capture reads `#groceries: milk ,eggs,  bread`
- **THEN** the items are `milk`, `eggs` and `bread`

#### Scenario: Order is the order written
- **WHEN** a list names `milk`, then `eggs`
- **THEN** `milk` orders before `eggs`

#### Scenario: A trailing separator names nothing
- **WHEN** a capture reads `#groceries: milk, eggs,`
- **THEN** the items are `milk` and `eggs`

### Requirement: A list gathers items and tagged tasks together

Reading a list SHALL return both the items expanded from list captures carrying
that tag and any task carrying that tag, as one sequence. A task SHALL NOT be
duplicated by appearing in more than one list.

#### Scenario: A task tagged by hand appears on the list
- **WHEN** a task carries the tag `groceries`
- **AND** the list `groceries` is read
- **THEN** that task appears alongside the expanded items

#### Scenario: A task in two lists exists once
- **WHEN** a task carries the tags `groceries` and `hardware`
- **THEN** one task exists
- **AND** it appears when either list is read

#### Scenario: Two list captures under one tag read as one list
- **WHEN** two captures each name items under the tag `groceries`
- **THEN** reading that list returns the items of both

### Requirement: Completing an item promotes it to a task and marks it done

Completing a list item SHALL promote that item to a task and mark that task done.
The item SHALL NOT be removed from the projection and the list capture SHALL NOT
be rewritten. Reopening SHALL mark the same task open again.

#### Scenario: Ticking an item records what happened
- **WHEN** the item `milk` is completed
- **THEN** a task exists for that item carrying the tag of its list
- **AND** that task is done
- **AND** the list capture's text is unchanged

#### Scenario: A completed item is still on the list
- **WHEN** an item has been completed
- **AND** its list is read
- **THEN** the item is returned, marked done

#### Scenario: Reopening is the same path backwards
- **WHEN** a completed item is reopened
- **THEN** its task is open
- **AND** no second task exists for that item

#### Scenario: Completing twice is completing once
- **WHEN** an item already completed is completed again
- **THEN** one task exists for that item
- **AND** it is done
