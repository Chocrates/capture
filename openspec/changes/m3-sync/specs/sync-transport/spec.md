## ADDED Requirements

### Requirement: Everything on the wire is inside a Noise session

The system SHALL carry every byte exchanged between devices inside an
established Noise_KK session. There SHALL be no unauthenticated pre-handshake
message, and no fallback to plaintext when a handshake fails.

This is stated as an absolute because it is violated by convenience rather than
by intent: the blob plane moves large byte ranges, and serving those over a
plain socket would put audio in the clear on the LAN while the note describing
it is encrypted.

#### Scenario: A failed handshake yields nothing
- **WHEN** a peer cannot complete the Noise_KK handshake
- **THEN** the connection SHALL be closed
- **AND** no application message SHALL have been sent or accepted on it

#### Scenario: No message escapes the session
- **WHEN** any message type is exchanged — `HELLO`, `RANGE`, `EVENT`, `REDACT`,
  `REDACT_ACK`, `WANT`, `BLOB`, `WANT_TGT`
- **THEN** it SHALL be a Noise transport message inside an established session

### Requirement: Mutual authentication from keys already held

The system SHALL authenticate both sides using the static public keys the
roster already contains, with no certificate authority and no certificate
lifecycle. A device that is not on the roster SHALL NOT be able to complete a
handshake, and SHALL NOT be able to read, inject, or observe anything beyond
connection metadata.

#### Scenario: A device that is not on the roster
- **WHEN** a device whose key is absent from the roster attempts to connect
- **THEN** the handshake SHALL fail
- **AND** the archive SHALL be unchanged

### Requirement: Frames fit inside a Noise message

The system SHALL cap frames at the Noise transport limit of 65535 bytes
including the 16-byte AEAD tag, giving 65519 usable. Blob chunks SHALL be
32 KiB so a chunk fits in one message with room for framing. Sessions SHALL
rekey on the standard interval so nonces cannot be exhausted on a long-lived
link doing a large transfer.

#### Scenario: A chunk fits
- **WHEN** a blob chunk is framed for transmission
- **THEN** the resulting message SHALL be within the Noise transport limit
