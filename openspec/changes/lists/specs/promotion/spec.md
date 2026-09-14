## ADDED Requirements

### Requirement: Completing a list item is a promotion, never a deletion

Completing a list item SHALL append a promotion making that item a task, and
SHALL mark that task done. Nothing SHALL be removed from the projection and the
list capture SHALL NOT be rewritten, on the same terms as §3.1.3's additive
promotion.

The promotion SHALL carry the tag of the item's list, so that the resulting task
is gathered by that list on the same footing as a task tagged by hand.

#### Scenario: Buying the milk is a fact with a time on it
- **WHEN** the item `milk` is completed
- **THEN** a promotion to a task is appended
- **AND** the task is done
- **AND** the task carries the tag of its list

#### Scenario: The capture is not rewritten
- **WHEN** an item is completed
- **THEN** the list capture's text is unchanged
- **AND** the item is still expanded from it

#### Scenario: Completion is idempotent
- **WHEN** an item already completed is completed again
- **THEN** one task exists for that item
- **AND** it is done

### Requirement: A promoted item is the same item, not a copy

An item promoted by completion SHALL remain the item its list expands, and
SHALL NOT additionally appear as a loose task on that list. Reading the list
SHALL return it once.

#### Scenario: A completed item is returned once
- **WHEN** an item has been completed
- **AND** its list is read
- **THEN** it appears exactly once, marked done

#### Scenario: A human correction is not overridden by a rebuild
- **WHEN** an item has been promoted by completion
- **AND** the projection is rebuilt
- **THEN** the promotion stands
- **AND** the item is still marked done
