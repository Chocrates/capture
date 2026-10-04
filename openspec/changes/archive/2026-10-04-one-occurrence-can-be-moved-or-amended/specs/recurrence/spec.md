## ADDED Requirements

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
