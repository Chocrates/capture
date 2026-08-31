## ADDED Requirements

### Requirement: Pairing and peer state are visible from the command line

The command line SHALL be able to pair a device, list peers with when each was
last reached, and report what is outstanding against each. Every read SHALL
take `--json`.

#### Scenario: Pairing a second device
- **WHEN** the user runs the pairing command on both devices
- **THEN** a `device_add` SHALL be appended
- **AND** both devices SHALL report the other as a peer

#### Scenario: Asking about peers
- **WHEN** the user asks for peers
- **THEN** each SHALL be listed with when it was last reached

### Requirement: A peer that has not been reached is reported loudly

A peer not reached in over a week SHALL be reported as needing attention
(§4.5 tier 1). It is the leading indicator of the failure this milestone exists
to prevent: a phone that has silently stopped syncing holds captures that exist
on exactly one device.

#### Scenario: A phone that stopped syncing
- **WHEN** a paired peer has not been reached for over a week
- **THEN** the status surface SHALL report it as needing attention
- **AND** SHALL say when it was last reached rather than only that it is stale

#### Scenario: A peer reached this morning
- **WHEN** a peer was reached within the week
- **THEN** it SHALL NOT be reported as needing attention
