## ADDED Requirements

### Requirement: Items may be written with or without a tag

A capture SHALL be read as a checklist when it writes items, whether or not it
names a tag.

The guards that keep the form from firing inside ordinary writing SHALL stand:
the form is anchored at the start of the capture, a tag where one is written is
a single slug, and something must follow. A match firing inside prose is worse
than no match at all (§3.1.5).

#### Scenario: A tagged checklist
- **WHEN** a capture reads `#shop: milk, eggs, bread`
- **THEN** three items SHALL be recognised under the tag `shop`

#### Scenario: An untagged checklist
- **WHEN** a capture writes items with no tag
- **THEN** the items SHALL be recognised
- **AND** no tag SHALL be recorded

#### Scenario: Prose is still prose
- **WHEN** a capture reads `ask sarah about #shop: she has it`
- **THEN** no checklist SHALL be recognised

#### Scenario: A clock time is not a tag and a colon
- **WHEN** a capture reads `#standup at 9:30`
- **THEN** no checklist SHALL be recognised
