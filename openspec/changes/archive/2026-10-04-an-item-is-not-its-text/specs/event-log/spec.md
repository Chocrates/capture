## ADDED Requirements

### Requirement: Item completion names a position

`item_done` and `item_undone` SHALL identify the item by its position in the
capture they target. They SHALL keep their kind numbers, and the body SHALL
retain the item's text as written so that a log dump remains legible.

An event written before this names its item only by text. Such an event SHALL be
applied where that text still matches an item of the capture, and SHALL be held
with a recorded reason where it does not — never silently dropped, and never
applied to a different item because the words happen to match it now.

#### Scenario: A completion survives its words changing
- **WHEN** an item is ticked and the capture is later edited to reword it
- **THEN** the completion SHALL still apply to that item

#### Scenario: An older event whose text still matches
- **WHEN** an event written before this change names an item that still exists
- **THEN** it SHALL apply to that item

#### Scenario: An older event whose text no longer matches
- **WHEN** an event written before this change names text no item now has
- **THEN** it SHALL be held with a recorded reason
- **AND** SHALL NOT be applied to any other item
