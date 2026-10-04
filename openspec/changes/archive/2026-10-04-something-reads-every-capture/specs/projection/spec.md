## ADDED Requirements

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
