# sync-transport Specification

## Purpose
TBD - created by archiving change m3-sync. Update Purpose after archive.
## Requirements
### Requirement: Everything on the wire is inside a Noise session

The system SHALL carry every byte exchanged between devices inside an
established Noise_IK session. There SHALL be no unauthenticated pre-handshake
message, and no fallback to plaintext when a handshake fails.

This is stated as an absolute because it is violated by convenience rather than
by intent: the blob plane moves large byte ranges, and serving those over a
plain socket would put audio in the clear on the LAN while the note describing
it is encrypted.

#### Scenario: A failed handshake yields nothing
- **WHEN** a peer cannot complete the Noise_IK handshake
- **THEN** the connection SHALL be closed
- **AND** no application message SHALL have been sent or accepted on it

#### Scenario: No message escapes the session
- **WHEN** any message type is exchanged — `HELLO`, `RANGE`, `EVENT`, `REDACT`,
  `REDACT_ACK`, `WANT`, `BLOB`, `WANT_TGT`
- **THEN** it SHALL be a Noise transport message inside an established session

### Requirement: Mutual authentication from keys already held

The system SHALL authenticate both sides using the static public keys the
roster already contains, with no certificate authority and no certificate
lifecycle. A device that has never held the archive SHALL NOT be able to
complete a handshake, and SHALL NOT be able to read, inject, or observe
anything beyond connection metadata.

**IK rather than the KK this document first specified.** KK requires both
static keys to be known before the handshake begins. That is true of the side
that dials, which chose who to call, and false of the side that answers: it
has a connection and no idea who opened it, cannot ask — the rule above
forbids any message outside a session — and cannot try each member in turn,
because a failed attempt consumes the connection. KK is not implementable for
an inbound connection in this topology, which was discovered by writing the
daemon rather than by reading this.

Under IK the initiator must still know the responder's static key, and that
key lives in the log, so a device without the archive cannot produce a valid
first message. The responder learns the initiator's key from the handshake and
checks it against the roster.

#### Scenario: A device that has never held the archive
- **WHEN** a device that is not and never was a member attempts to connect
- **THEN** it SHALL be unable to produce a valid first handshake message
- **AND** the archive SHALL be unchanged

#### Scenario: A retired device
- **WHEN** a device whose key has been retired connects
- **THEN** it SHALL complete the handshake, because it still holds the archive
- **AND** the session SHALL be refused on the roster check before any message
  is accepted

### Requirement: A re-admission has to be written by a member

The system SHALL allow a `device_readmit` to be authored by a device that is
still a member, and SHALL do so by default when one is available.

A retired device cannot open a session — that is what retiring it achieved —
so a re-admission it authored has no route to the devices refusing it. Without
this, the remedy could not travel through the refusal it exists to undo. The
carry-a-signature-between-machines flow remains for the lockout case, where
every device has been retired and none can author anything.

#### Scenario: Re-admitting from a device that is still a member
- **WHEN** a member vouches for a retired device
- **THEN** it SHALL author the `device_readmit` itself
- **AND** that event SHALL replicate as any other event does

### Requirement: Frames fit inside a Noise message

The system SHALL cap frames at the Noise transport limit of 65535 bytes
including the 16-byte AEAD tag, giving 65519 usable. Blob chunks SHALL be
32 KiB so a chunk fits in one message with room for framing. Sessions SHALL
rekey on the standard interval so nonces cannot be exhausted on a long-lived
link doing a large transfer.

#### Scenario: A chunk fits
- **WHEN** a blob chunk is framed for transmission
- **THEN** the resulting message SHALL be within the Noise transport limit

