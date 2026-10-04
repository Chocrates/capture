## ADDED Requirements

### Requirement: Every target of a redaction is queued

The queue of pending destructions SHALL hold one entry per target, not one per
redaction. A redaction naming several notes names them all, and a queue that
can hold only one of them destroys one and reports success.

#### Scenario: A redaction naming three notes queues three
- **WHEN** one redaction names three notes
- **THEN** three destructions are queued

#### Scenario: Every named note is destroyed
- **WHEN** that redaction is carried out
- **THEN** none of the three keeps its content

#### Scenario: One redaction per note is unaffected
- **WHEN** three separate redactions each name one note
- **THEN** three destructions are queued, as before
