## ADDED Requirements

### Requirement: Live push is an optimisation and may fail silently

The system SHALL treat live push as a latency optimisation. A failed push SHALL
NOT be an error, SHALL NOT be retried indefinitely, and SHALL NOT be the only
path by which an event reaches a peer.

Nothing SHALL depend on a push having succeeded. Anything that does is a bug
that appears only when the network is bad, which is when it is hardest to see.

#### Scenario: A peer that is not reachable
- **WHEN** a push cannot be delivered
- **THEN** the append SHALL be unaffected
- **AND** the event SHALL reach the peer by anti-entropy instead

### Requirement: A redaction is pushed until acknowledged

A `redact` SHALL be pushed eagerly and retried until acknowledged. For
redaction, delivery latency is the guarantee rather than a nicety (§1.1): the
window between ordering a destruction and its taking effect everywhere is
exactly the exposure the feature exists to close.

An outstanding redaction against a peer that has not acknowledged it SHALL be
reported rather than assumed complete.

#### Scenario: A peer is asleep when a redaction is ordered
- **WHEN** a `redact` is appended and a peer is unreachable
- **THEN** the system SHALL retry until that peer acknowledges
- **AND** SHALL report the redaction as outstanding against it meanwhile

#### Scenario: A peer that is gone for good
- **WHEN** a peer is retired without ever acknowledging a redaction
- **THEN** the system SHALL continue to report it as never acknowledged
- **AND** SHALL NOT report the redaction as complete
