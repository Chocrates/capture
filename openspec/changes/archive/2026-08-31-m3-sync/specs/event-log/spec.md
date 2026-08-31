## ADDED Requirements

### Requirement: Events from a peer enter through the same door as local ones

The system SHALL ingest events received from a peer by the same path that
appends local ones: signature verification, `bhash` checking, chain and sequence
rules, and the quarantine rules for unknown kinds SHALL apply unchanged.

There SHALL NOT be a second ingest path optimised for batches. Two ways into the
log is two sets of rules about what may enter it, and the second set is the one
nobody remembers to update.

#### Scenario: A malformed event from a peer
- **WHEN** an event arrives over the wire whose `bhash` does not match its body
- **THEN** it SHALL be rejected exactly as a local one would be
- **AND** SHALL be reported the same way

#### Scenario: An unknown kind from a newer peer
- **WHEN** an event arrives with a kind this build does not know
- **THEN** it SHALL be held rather than discarded
- **AND** SHALL be re-evaluated after an upgrade

### Requirement: An origin's stream tolerates arriving out of order

An origin's events SHALL be gap-free in `seq`, and the system SHALL hold events
that arrive ahead of their predecessors rather than folding a stream with a hole
in it or rejecting the event outright.

#### Scenario: A range arrives out of order
- **WHEN** `desktop:41520` arrives before `desktop:41519`
- **THEN** the fold SHALL NOT advance past 41518 until 41519 arrives
- **AND** 41520 SHALL be retained rather than requested again

#### Scenario: The same event arrives twice
- **WHEN** an event already held arrives again from another peer
- **THEN** it SHALL be ignored
- **AND** the fold SHALL be unchanged

### Requirement: A peer cannot write to another origin's stream

The system SHALL reject an event whose signature does not match the origin it
claims, whatever peer relayed it. Relaying SHALL NOT confer authority.

#### Scenario: A relayed event with a bad signature
- **WHEN** a peer relays an event signed by a key that is not the claimed
  origin's
- **THEN** it SHALL be rejected
- **AND** SHALL be reported as a rejected event (§4.5 tier 1)
