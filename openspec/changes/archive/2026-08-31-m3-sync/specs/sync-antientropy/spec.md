## ADDED Requirements

### Requirement: Reconciliation is a version-vector diff

A replica's state SHALL be described by one integer per origin, which is what
single-writer-per-origin with a gap-free `seq` buys. On session establishment
each side SHALL send `HELLO` carrying its vector, SHALL compute its own diff
without a negotiation round, and SHALL request what it lacks with `RANGE`.

The system SHALL NOT require a Merkle tree, a bloom filter, or a
set-difference estimator. That complexity was paid for in the data model in M1.

#### Scenario: One side is behind
- **WHEN** A holds `desktop: 41822` and B holds `desktop: 41500`
- **THEN** B SHALL request `desktop` from 41501 to 41822
- **AND** neither side SHALL need a round of negotiation to work that out

#### Scenario: Both sides have something
- **WHEN** each side holds events the other lacks
- **THEN** both ranges SHALL be exchanged in the same session

### Requirement: Convergence does not depend on any device being online

Any device SHALL relay any origin's events. The system SHALL NOT require a
particular device to be reachable for the archive to converge.

#### Scenario: The desktop is off
- **WHEN** the phone captures while the desktop is powered off
- **AND** the laptop syncs with the phone, and later with the desktop
- **THEN** the desktop SHALL hold the phone's events

### Requirement: Anti-entropy runs often enough to be the safety net

The system SHALL run anti-entropy on session open, on network change, on wake,
and on a timer while connected. Correctness SHALL rest on this rather than on
live push.

#### Scenario: A push that was never delivered
- **WHEN** a live push fails silently
- **THEN** the event SHALL still reach the peer at the next anti-entropy round

### Requirement: A sealed origin keeps its entry for ever

When an origin is sealed, the system SHALL retain a permanent entry recording
its final `seq` and the id of its final event. The entry SHALL NOT be dropped.

Dropping it breaks the completeness oracle: a newly paired device asking what it
is missing is told nothing for that origin, receives none of its events, and
reports agreement with every peer — so after two reinstalls a fresh device holds
a fraction of the archive and believes it is whole. It also makes an unreachable
redaction permanently bypassable, since a returning replica can hand over content
whose destroying `redact` lived in a stream the new device never received.

The cost is about forty bytes per retired origin, permanently, and is stated
rather than claimed away.

#### Scenario: A new device after a reinstall
- **WHEN** a device is paired into an archive that has a sealed origin
- **THEN** it SHALL be able to discover that the sealed origin's events are
  missing
- **AND** SHALL be able to request them in one range

#### Scenario: Sealing names its roster
- **WHEN** a seal is computed
- **THEN** it SHALL name the roster it was computed against
- **AND** SHALL be honoured only by devices that agree that roster is current
