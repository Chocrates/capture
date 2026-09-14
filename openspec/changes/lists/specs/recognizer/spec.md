## ADDED Requirements

### Requirement: The list form is tier 1 and adds no vocabulary

The recognizer SHALL read a tag sigil, a slug and a colon at the start of a
capture as the opening of a list. The form SHALL be recognised in tier 1, SHALL
NOT be scored, and SHALL NOT be overridden by any inference in the same line.

The sigil table SHALL NOT gain a character and the closed vocabulary of §3.1.5
SHALL NOT gain a word. The form is punctuation over an existing sigil rather
than new vocabulary, so the exhaustiveness of the tier 2 tests is unaffected.

#### Scenario: A list beats an inference in the same line
- **WHEN** a capture reads `#groceries: buy milk tomorrow`
- **THEN** a list carrying the tag `groceries` is recognised
- **AND** no task is inferred from `buy` and `tomorrow`
- **AND** no suggestion of either is recorded

#### Scenario: The vocabulary is unchanged
- **WHEN** the recognised vocabulary is enumerated
- **THEN** it holds the same words as before this change
- **AND** the sigil table holds the same characters

### Requirement: Guards prevent a colon in prose from opening a list

The list form SHALL match only where the sigil is the first thing in the
capture. A sigil appearing after any other token SHALL be read as an ordinary
tag, whatever punctuation follows it.

A capture SHALL NOT be read as a list where nothing follows the colon but
whitespace.

#### Scenario: A mid-line sigil is a tag
- **WHEN** a capture reads `reminder about #groceries: ask first`
- **THEN** no list is recognised
- **AND** the tag `groceries` is carried as an ordinary tag

#### Scenario: A time is not a list
- **WHEN** a capture reads `#standup at 9:30`
- **THEN** no list is recognised
- **AND** the tag `standup` is carried as an ordinary tag

#### Scenario: Nothing after the colon is not a list
- **WHEN** a capture reads `#groceries:   `
- **THEN** no list is recognised

### Requirement: Recognising a list never delays or blocks capture

A capture SHALL be committed before the list form is attempted, on the same
terms as every other recognition. A failure to parse the item sequence SHALL
leave an ordinary note carrying the tag and SHALL NOT be reported as an error.

#### Scenario: Commit does not wait for expansion
- **WHEN** a list capture is committed
- **THEN** the commit returns before items are expanded

#### Scenario: A parse failure loses nothing
- **WHEN** the item sequence cannot be read
- **THEN** the capture is stored exactly as typed
- **AND** it carries the tag
- **AND** nothing is reported as having failed
