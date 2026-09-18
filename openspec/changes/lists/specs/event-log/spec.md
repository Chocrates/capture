## ADDED Requirements

### Requirement: Completing a list item has its own event kinds

The log SHALL carry two kinds for list item completion: one recording that an
item is done, and one reversing it. Each SHALL name the list capture it is
about and the item's text within it.

The item is named by its **text**, not by its position. Positions shift when a
list is edited — inserting an item moves every item after it — and completion
bound to a position would silently move to a different item. This is the same
reason an occurrence is named by the instant its rule would have produced
rather than by its index (§3.5).

A device that does not recognise these kinds SHALL hold them rather than fail,
as it does for any unrecognised kind.

#### Scenario: An item is named by its text
- **WHEN** a completion event is written for the item `milk`
- **THEN** it names the list capture and the text `milk`

#### Scenario: A tick follows its item through an edit
- **WHEN** an item is completed
- **AND** the list capture is edited so that the item moves position
- **THEN** the same item is still completed
- **AND** no other item became completed

#### Scenario: An older device holds what it cannot read
- **WHEN** a device that predates these kinds receives one
- **THEN** it holds the event rather than rejecting it
- **AND** folding continues
