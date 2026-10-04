## ADDED Requirements

### Requirement: An ordered destruction withholds every table derived from the text

A capture whose destruction has been ordered SHALL contribute nothing to any
table derived from its text, for the whole cancellation window, and SHALL do so
by one condition rather than one per table.

§1.1 withholds the content when the destruction is **ordered**, not when it is
carried out. The window exists so a misfire can be cancelled, which is why the
fold keeps the text and records the order beside it — withholding in the fold
would make cancellation unable to restore anything. Deciding what a reader may
see is the projection's job, and it is one decision.

It was four, and two of them were wrong. `notes.text` and the alertable set each
withhold centrally, and each carries a comment recording that a hand-written
copy of this rule had been present in some call sites and absent from the rest.
The search index withholds on the write path and names the case exactly. The
mentions and the list items, in the same function, were guarded only by the text
being present at all — true for the entire window — so a note somebody had asked
to destroy went on naming people and listing its contents for thirty minutes.

A table emptied by this rule SHALL be emptied, not merely left unwritten. Rows
already present from before the order are the ones that matter, and a guard that
only skips the insert leaves them in place.

#### Scenario: A note queued for destruction names nobody
- **WHEN** a note's destruction is ordered and the window has not elapsed
- **THEN** it SHALL contribute no mentions
- **AND** SHALL NOT appear on any entity page

#### Scenario: A note that already named somebody stops
- **WHEN** a note whose mentions were already projected has its destruction ordered
- **THEN** the mentions SHALL be removed, not merely left unrecomputed

#### Scenario: A queued list withholds its items
- **WHEN** a capture that wrote items has its destruction ordered
- **THEN** it SHALL contribute no items

#### Scenario: A queued note's links are withheld from both ends
- **WHEN** a note whose text holds a link has its destruction ordered
- **THEN** the link SHALL contribute no row
- **AND** the target's backlinks SHALL NOT name it

#### Scenario: A tag done to the note survives
- **WHEN** a tagged note has its destruction ordered
- **THEN** the tag SHALL still be recorded against it
- **AND** this SHALL NOT be treated as an inconsistency with the mention of the
  same word in its text: a tag is something done to a note and is built from
  its own events, while a mention is part of what the note says

#### Scenario: Cancelling restores what was withheld
- **WHEN** an ordered destruction is cancelled within the window
- **THEN** the mentions and the items SHALL be present again
- **AND** SHALL match what the capture says

### Requirement: No read path reaches around the withholding

A query answering a capture's items SHALL apply the same withholding as a query
answering its text, whether or not it reads the text column.

Withholding `notes.text` centrally is what makes every text read safe, and it is
why that constant exists. A table derived from the text is the same material
under another name: the items of a list are its words. Two queries answered a
list without going through that constant, so the words were readable by asking
for them in the shape they had been expanded into.

#### Scenario: Reading a queued capture's items directly
- **WHEN** a capture's destruction is ordered and its items are asked for by name
- **THEN** no items SHALL be answered

#### Scenario: Reading a queued capture's list by tag
- **WHEN** a list is read by its tag and one capture under it is queued for destruction
- **THEN** that capture's items SHALL be absent
- **AND** the other captures' items SHALL be unaffected
