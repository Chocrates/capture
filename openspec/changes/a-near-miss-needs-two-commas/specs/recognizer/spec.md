## MODIFIED Requirements

### Requirement: A capture that nearly matched a form says so

Where a capture misses the list form by one thing, the system SHALL be able to
say what was missing. Recognising a near miss SHALL NOT make it a match.

§3.1.5 keeps the guards strict because a form firing inside ordinary writing is
worse than one that never fires. That argument is about *acting* on a guess.
Saying "this looks like a list you meant" costs a glance, which is the price
§3.1.3 already sets for a suggestion, and the alternative is what happened: a
grocery list written as one, not recognised, and nothing anywhere saying why.

Two misses SHALL be recognised and no others:

- an anchored tag with no colon, where **at least two** commas appear after it;
- an anchored tag with a colon and nothing after it.

Two commas rather than one, and the difference is measured rather than argued.
One comma was the first rule, and against hand-written tagged prose it fired on
every line it was tried against — `#reading finished the long one, finally`,
`#health slept badly, again`, `#journal a quiet day, mostly reading` — six false
offers out of eight. A suggestion somebody does not want is one they learn to
ignore, and an ignored channel cannot deliver the one that matters. So the
threshold is part of the requirement, not an implementation detail: lowering it
to one comma is what this sentence exists to prevent.

A near miss SHALL NOT be reported where the capture already is a list, and SHALL
NOT be reported for text that is not anchored with a tag.

#### Scenario: A tag, commas, and no colon
- **WHEN** a capture reads `#groceries 7 apples, rye flour, 1kg lentils`
- **THEN** a near miss SHALL be available, naming the missing colon
- **AND** the capture SHALL still be a tagged note and not a list

#### Scenario: One comma is ordinary prose
- **WHEN** a capture reads `#health slept badly, again`
- **THEN** no near miss SHALL be reported
- **AND** the capture SHALL still be a tagged note

#### Scenario: A colon with nothing after it
- **WHEN** a capture reads `#groceries:`
- **THEN** a near miss SHALL be available
- **AND** the capture SHALL still be a tagged note

#### Scenario: A list is not a near miss
- **WHEN** a capture reads `#groceries: milk, eggs`
- **THEN** no near miss SHALL be reported

#### Scenario: Prose is not a near miss
- **WHEN** a capture reads `ask sarah about the invoice, she has it`
- **THEN** no near miss SHALL be reported

#### Scenario: A tag with no commas is an ordinary note
- **WHEN** a capture reads `#reading finished the long one`
- **THEN** no near miss SHALL be reported
