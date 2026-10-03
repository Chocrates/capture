## ADDED Requirements

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
