## ADDED Requirements

### Requirement: A misfired redaction can be recovered while its snapshot lives

The command line SHALL be able to list the snapshots taken before redactions
and still within their retention, and SHALL be able to bring one back.

A snapshot that cannot be read is a plaintext copy of content somebody ordered
destroyed, kept for thirty days, with its cost paid and its purpose unreachable.

#### Scenario: Listing what can be recovered
- **WHEN** a redaction has been carried out and its snapshot has not expired
- **THEN** it is listed, with when it was taken and what it begins with

#### Scenario: Nothing to recover
- **WHEN** no snapshot is within its retention
- **THEN** that is said plainly rather than shown as an empty list

#### Scenario: An expired snapshot is not offered
- **WHEN** a snapshot's retention has elapsed
- **THEN** it is not listed
- **AND** recovering it is refused

### Requirement: Recovery is a new capture, and says so

Recovering SHALL write the snapshot's text as a new capture, and SHALL say that
is what happened. It SHALL NOT present the redaction as undone: the body is gone
from the log, and the redaction is retained so that it destroys matching events
arriving later.

#### Scenario: Recovering brings the text back
- **WHEN** a snapshot is recovered
- **THEN** a new note carries its text
- **AND** the output says a new note was written rather than that the redaction was reversed

#### Scenario: The redaction still stands
- **WHEN** a snapshot has been recovered
- **THEN** the original note is still redacted
