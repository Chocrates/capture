## ADDED Requirements

### Requirement: Redaction destroys content held outside the log

Redaction SHALL destroy the bytes of any blob the redacted events reference,
not only the event bodies. A blob holds the content of a voice note or an
image; destroying the event body and leaving the recording tells somebody their
content is gone while it is on disk.

The bytes SHALL be overwritten before being unlinked, as §1.1 requires.

#### Scenario: A redacted voice note loses its audio
- **WHEN** a note carrying a blob reference is redacted
- **THEN** the blob's bytes are destroyed
- **AND** the event body is destroyed

#### Scenario: A note with no blob is unaffected
- **WHEN** a note carrying no blob reference is redacted
- **THEN** the redaction completes as before

### Requirement: A blob another note still references is kept

A blob SHALL NOT be destroyed while any capture whose body is intact still
references it. The same recording captured twice is one blob with two
references, and destroying it because one of them was redacted would destroy
content nobody asked to lose — which is the cardinal failure, performed by the
operation least able to undo it.

#### Scenario: Two notes share a recording and one is redacted
- **WHEN** two captures reference one blob
- **AND** one of them is redacted
- **THEN** the blob's bytes remain
- **AND** the surviving note can still reach them

#### Scenario: The last reference goes
- **WHEN** the second of two notes sharing a blob is redacted
- **THEN** the blob's bytes are destroyed

### Requirement: Blobs are destroyed before the bodies that name them

Blob destruction SHALL happen before body destruction, and SHALL have a
recorded phase of its own so an interrupted purge resumes into it. The blob hash
is carried in the capture body, so destroying the body first leaves nothing able
to say which blob the note referenced.

A crash between the two SHALL leave a state the next run can finish: a body that
still names a blob is recoverable, and a blob that nothing can name is not.

#### Scenario: Interrupted after the blobs and before the bodies
- **WHEN** a purge is interrupted after destroying blobs and before destroying bodies
- **THEN** the next run completes the purge
- **AND** destroying a blob that is already gone is not an error

#### Scenario: The phase is recorded
- **WHEN** a purge has destroyed the blobs of its target
- **THEN** that is recorded before the bodies are touched
