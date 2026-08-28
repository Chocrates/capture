# recognizer Specification

## Purpose
TBD - created by archiving change m2-types-and-review. Update Purpose after archive.
## Requirements
### Requirement: The three tiers are attempted in order and sigils always win

Recognition SHALL attempt tier 1 (sigils), then tier 2 (the grammar), and SHALL
leave anything both decline to tier 3 or to nothing. Where an explicit sigil and
an inferred expression disagree, the sigil SHALL win and the inference SHALL NOT
be recorded, not even as a suggestion.

#### Scenario: An explicit marker beats inferred text
- **WHEN** a capture reads `>fri` in a line that also says "thursday"
- **THEN** the promotion carries Friday
- **AND** no suggestion of Thursday is recorded

#### Scenario: Tier 2 declining is not an error
- **WHEN** the grammar recognises nothing in a capture
- **THEN** the capture remains an ordinary note
- **AND** nothing is reported as having failed

### Requirement: The recognised vocabulary is closed

Tier 2 SHALL recognise only the words enumerated in `DESIGN.md` §3.1.5. A word
outside that list SHALL NOT be matched approximately, and adding one SHALL be a
deliberate edit rather than a runtime inference.

#### Scenario: An unlisted word is not a near miss
- **WHEN** a capture contains a temporal word absent from the vocabulary
- **THEN** tier 2 does not match it
- **AND** the capture is left to tier 3 or remains a note

### Requirement: Guards prevent matches inside ordinary prose

Recognition SHALL apply the guards of `DESIGN.md` §3.1.5 regardless of what the
grammar alone would accept: an article before a month, weekday or bare clock
prevents a match; a bare number is never a time without a colon, a meridiem or a
marker; a following noun prevents a bare weekday or month matching; and a match
must begin at the head of the capture, at a comma, or after a marker.

#### Scenario: A month inside a noun phrase is not a date
- **WHEN** a capture reads "the march timeline slipped"
- **THEN** nothing is promoted
- **AND** no date suggestion is recorded

#### Scenario: A bare number is not a time
- **WHEN** a capture reads "budget 40k, revisit in Q2"
- **THEN** no time of 40:00 is produced
- **AND** the Q2 marker is recognised on its own terms

#### Scenario: A time behind an article is prose
- **WHEN** a capture reads "the 7am problem"
- **THEN** nothing is promoted

### Requirement: Confidence decides promotion, and the floor is 0.75

Every recognition SHALL carry a confidence score. At or above 0.75 it SHALL
promote. Below it SHALL be recorded as a suggestion surfaced as one line in
review, and SHALL NOT promote.

#### Scenario: An unambiguous datetime promotes
- **WHEN** a capture reads "dentist thursday 2pm"
- **THEN** it is promoted to a timed event

#### Scenario: A bare weekday only suggests
- **WHEN** a capture reads "dentist thursday"
- **THEN** nothing is promoted
- **AND** a suggestion is available in review

#### Scenario: A task verb alone only suggests
- **WHEN** a capture reads "send sarah the migration doc" with no temporal expression
- **THEN** nothing is promoted
- **AND** a suggestion that this may be a task is available in review

### Requirement: Recognition never delays or blocks capture

Parsing SHALL happen after the capture is committed. The overlay SHALL close on
Enter regardless of whether anything can be recognised, and there SHALL be
nothing to confirm, accept or dismiss at capture time.

#### Scenario: Enter commits with no parser available
- **WHEN** the chord is used while nothing can parse the text
- **THEN** the capture is stored and the window closes
- **AND** the capture latency budget is unaffected

#### Scenario: A parse failure loses nothing
- **WHEN** parsing fails entirely for a capture
- **THEN** the note is still stored, searchable and surfaced

