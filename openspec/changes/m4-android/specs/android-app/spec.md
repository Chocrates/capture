## ADDED Requirements

### Requirement: The phone holds the whole archive

The Android application SHALL hold the complete event log and its own projection, and SHALL
reconcile with peers by the same anti-entropy every other device uses.

§5.5 is explicit that no device is required to be online. A phone that held only a window onto the
desktop's archive would make the desktop a server in everything but name, and would be useless
exactly when it is the only device present.

#### Scenario: The phone is the only device present
- **WHEN** the desktop is switched off
- **THEN** the phone SHALL be able to capture, read, search and complete work
- **AND** SHALL reconcile when the desktop returns

### Requirement: The phone reads everything the desktop reads

The application SHALL offer timeline, note page, search, agenda, wiki with nesting, calendar,
review and health.

A device that can only write sends its user to another machine to read what they wrote a moment
ago. §8.4 rejects that gradient for the desktop; it applies with more force to the device most
often within reach.

#### Scenario: Reading a note that was captured elsewhere
- **WHEN** a note captured on the desktop has reached the phone
- **THEN** it SHALL be readable, searchable and answerable from the phone alone

### Requirement: The phone does not enrich and does not transcribe

The application SHALL NOT run transcription, entity extraction, or any other enrichment.

Voice capture commits a blob reference and stops there. Android background execution limits make
enrichment unreliable, and the phone's job ends at "the audio exists" (§2.2).

#### Scenario: A voice capture
- **WHEN** audio is captured on the phone
- **THEN** the audio SHALL be stored and referenced by an event
- **AND** no transcript SHALL be produced on the phone
- **AND** the transcript SHALL arrive later as an enrichment authored by a device that does that
  work
