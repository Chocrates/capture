## ADDED Requirements

### Requirement: Capture is reachable without opening the application

The phone SHALL offer a Quick Settings tile reachable from the lock screen, a share-sheet target,
and a hold-to-talk gesture. Each SHALL commit without the application being brought to the
foreground first.

§2.1's whole argument is that capture must cost almost nothing at the moment a thought arrives. A
surface that requires unlocking, finding an icon and waiting for a cold start has already lost the
thought it was meant to catch.

#### Scenario: Capturing from the lock screen
- **WHEN** the tile is used from the lock screen
- **THEN** the text SHALL be captured and committed
- **AND** the application SHALL NOT need to have been opened

#### Scenario: Capturing from another application
- **WHEN** text is shared to the share-sheet target
- **THEN** it SHALL be captured
- **AND** the source URL SHALL be attached where the sharing application provides one

### Requirement: Voice ends at the audio

A voice capture SHALL record locally and commit a blob reference immediately. It SHALL NOT wait for
transcription, and SHALL NOT block on the network.

#### Scenario: Voice captured with no network
- **WHEN** audio is captured while the phone is offline
- **THEN** the capture SHALL be committed
- **AND** the audio SHALL be held until it can be replicated

### Requirement: A capture the interface accepted is never lost

If the store cannot be written, the capture SHALL be spooled and committed later, and the interface
SHALL NOT report success for something that was not durably held somewhere.

This is the same promise the desktop makes, and it matters more here: a phone is killed by the
platform routinely and without warning.

#### Scenario: The store cannot be written
- **WHEN** a capture is made and the store is unwritable
- **THEN** the capture SHALL be spooled
- **AND** SHALL appear in the archive once the store is available
- **AND** the condition SHALL be reported rather than passing silently
