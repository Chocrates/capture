## ADDED Requirements

### Requirement: Whether a peer has gone quiet is one decision

Every surface SHALL decide that a peer has gone quiet by the same rule, held in
one place, and SHALL report it rather than each deciding for itself.

A device gone quiet holds captures that exist on exactly one machine, and the
longer that goes unnoticed the more of them there are. Two surfaces answering
that question separately can answer it differently, and when they do neither is
obviously wrong — which makes a reader stop trusting the report rather than the
surface.

A peer never reached SHALL count as quiet. There is no moment to measure from,
and the captures on it are in exactly the state the report exists to name.

How long is "quiet" SHALL be a stated span rather than a number repeated where
it is used.

#### Scenario: Reached inside the span
- **WHEN** a peer was last reached less than the span ago
- **THEN** it SHALL NOT be reported as quiet

#### Scenario: Reached outside the span
- **WHEN** a peer was last reached longer than the span ago
- **THEN** it SHALL be reported as quiet, wanting attention
- **AND** the report SHALL say how long it has been

#### Scenario: Never reached at all
- **WHEN** a peer has never been reached
- **THEN** it SHALL be reported as quiet
- **AND** the report SHALL say that it has never been reached, rather than a duration

#### Scenario: Both surfaces agree
- **WHEN** the terminal and the window are asked about the same peer
- **THEN** they SHALL agree about whether it is quiet
