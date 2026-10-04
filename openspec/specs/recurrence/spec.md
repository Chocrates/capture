# recurrence Specification

## Purpose
TBD - created by archiving change m2-types-and-review. Update Purpose after archive.
## Requirements
### Requirement: The rule is stored and occurrences never are

A recurring series SHALL store its rule as an RFC 5545 subset — `FREQ`,
`INTERVAL`, `BYDAY`, `BYMONTHDAY`, `COUNT`, `UNTIL` — and SHALL NOT write
occurrences to the event log. Truth is the rule plus its exceptions.

#### Scenario: A daily habit costs one event
- **WHEN** a series recurring daily with no end is stored
- **THEN** the log holds one event for the rule
- **AND** no occurrence is written to the log at any point

#### Scenario: An unbounded series is legal
- **WHEN** a series is stored with neither `UNTIL` nor `COUNT`
- **THEN** it is accepted
- **AND** it costs one row per occurrence inside the expansion window and nothing in the log

### Requirement: Occurrences are derived projection data over a rolling horizon

Occurrences SHALL be expanded lazily into the projection over a rolling horizon,
SHALL be extended in the background and on view access, and SHALL be fully
rebuildable. They are never authoritative.

#### Scenario: A rebuild reproduces them
- **WHEN** the projection is rebuilt from the log
- **THEN** the expanded occurrences are identical to those discarded

#### Scenario: An exhausted horizon is not missing data
- **WHEN** a device has been asleep long enough for the expansion window to run out
- **THEN** the horizon is extended on first access
- **AND** no occurrence is missing or wrong, because the rule is the truth

### Requirement: An occurrence is named by series and original start

An occurrence SHALL be identified by `(series_id, original_start)`, where
`original_start` is the instant the rule would have produced before any move.
This gives a stable name to an occurrence that has never been materialised.

#### Scenario: A future occurrence can be completed before it exists anywhere
- **WHEN** next Tuesday's instance of a series is completed
- **THEN** the completion names the series and the original start
- **AND** it succeeds without any row for that occurrence having been written first

#### Scenario: A moved occurrence keeps its name
- **WHEN** an occurrence is moved to a new time
- **THEN** it is still identified by its original start
- **AND** a later exception naming that original start applies to it

### Requirement: Exception events modify one occurrence without touching the series

`occ_move`, `occ_edit`, `occ_cancel`, `occ_done` and `occ_undone` SHALL carry
`tgt = series_id` and `occ = original_start`, and SHALL affect only that
occurrence.

#### Scenario: Skipping one instance leaves the series intact
- **WHEN** one occurrence is cancelled
- **THEN** that occurrence is not shown
- **AND** every other occurrence of the series is unaffected

#### Scenario: A cancelled occurrence is recoverable
- **WHEN** an occurrence has been cancelled
- **THEN** the cancellation is an ordinary event in the log
- **AND** nothing about the occurrence has been destroyed

### Requirement: Changing this-and-future splits the series

Editing a recurring series with the scope "this and future" SHALL bound the
existing rule with `UNTIL` immediately before the occurrence, create a new
series starting at that occurrence, and link the two by `split_from`. The rule
SHALL NOT be edited in place for that scope.

#### Scenario: The past is left as it happened
- **WHEN** a weekly series is changed from Tuesdays to Thursdays from next week
- **THEN** occurrences before the split still fall on Tuesdays
- **AND** occurrences from the split fall on Thursdays

#### Scenario: All-occurrences scope edits the rule
- **WHEN** the same series is edited with the scope "all occurrences"
- **THEN** the rule itself changes and past occurrences change with it

#### Scenario: Exceptions survive a split
- **WHEN** a series carrying occurrence exceptions is split
- **THEN** each exception binds to whichever series covers its original start

### Requirement: One occurrence can be moved without touching the series

Moving a single occurrence SHALL change when that occurrence happens and nothing
else: the rule SHALL be untouched, and every other occurrence SHALL keep its
time.

This is §3.5's first edit scope. The other two are different mechanisms and must
stay different: changing the rule moves past occurrences too, and "this and
future" is a split. Collapsing them is the failure §3.5 names — editing the rule
in place silently rewrites what already happened.

A moved occurrence SHALL keep its name. An occurrence is named by its original
start, so moving it is a statement about when it happens and not about which
occurrence it is — which is what lets a completion written before the move still
apply to it afterwards.

Moving SHALL be refused for a note that has no occurrences, naming what produces
them. An exception naming a plain note would sit in the log for ever recording a
mistake that changes nothing when folded.

#### Scenario: An occurrence moves and its siblings do not
- **WHEN** one occurrence of a series is moved to a new time
- **THEN** it SHALL be shown at the new time
- **AND** every other occurrence SHALL be unchanged
- **AND** the series rule SHALL be unchanged

#### Scenario: A move keeps the occurrence's identity
- **WHEN** an occurrence is completed and then moved
- **THEN** it SHALL still be complete at its new time

#### Scenario: Moving something with no occurrences is refused
- **WHEN** a move names a note that is not a series
- **THEN** it SHALL be refused, saying what produces occurrences
- **AND** no event SHALL be written

#### Scenario: A move onto a DST transition is the person's choice, not a shift
- **WHEN** an occurrence is moved to a time that does not exist, or happens twice, in the local zone
- **THEN** it SHALL be placed where it was asked to go
- **AND** it SHALL NOT be marked as shifted, because nothing moved it on its own

### Requirement: One occurrence can carry its own details

A single occurrence SHALL be able to carry a title, a location and notes of its
own, overriding the series' for that occurrence only.

The capability is stated in §3.5's exception table and has had storage, a body
type and a fold arm since M2 with nothing able to author one — so the columns
were always empty and the override was unreachable. A capability nothing can
reach is not a capability.

Amending SHALL be refused for a note with no occurrences, on the same terms as
moving.

#### Scenario: An amended occurrence differs from its siblings
- **WHEN** one occurrence is given its own title
- **THEN** that occurrence SHALL show the new title
- **AND** every other occurrence SHALL show the series' own

#### Scenario: Amending does not move anything
- **WHEN** an occurrence is amended
- **THEN** its time SHALL be unchanged

#### Scenario: An exception survives a split
- **WHEN** an occurrence is amended and the series is later split from a point before it
- **THEN** the amendment SHALL still apply to that occurrence

