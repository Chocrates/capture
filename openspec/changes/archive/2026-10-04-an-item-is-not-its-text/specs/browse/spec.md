## ADDED Requirements

### Requirement: A capture's checklist is reachable from the capture

Where a surface shows a capture that wrote items, it SHALL offer that capture's
own checklist, and ticking from there SHALL be the same write as ticking
anywhere else.

A collection under a tag SHALL remain available and SHALL be presented as what
it is: everything carrying that tag, from however many captures.

#### Scenario: Ticking from the capture
- **WHEN** a capture's own items are shown
- **THEN** each SHALL be tickable
- **AND** the result SHALL be identical to ticking it from the tag's collection

#### Scenario: A collection says how many captures it gathers
- **WHEN** a tag's collection holds items from more than one capture
- **THEN** the surface SHALL make clear they are not one capture's list
