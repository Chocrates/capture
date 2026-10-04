## ADDED Requirements

### Requirement: The redaction confirmation names a recording it will destroy

Where a note being redacted carries a recording, the confirmation SHALL say so.
§1.1 makes this the one confirmation in the system that must not be trained
away, and it can only be that if it names what actually goes — a person who
would hesitate over an audio recording should not have to infer that it is
included.

#### Scenario: Redacting a note with a recording
- **WHEN** redaction is confirmed for a note carrying a blob
- **THEN** the prompt says the recording will be destroyed too

#### Scenario: Redacting a note without one
- **WHEN** the note carries no blob
- **THEN** the prompt does not mention a recording
