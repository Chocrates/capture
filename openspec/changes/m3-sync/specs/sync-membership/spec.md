## ADDED Requirements

### Requirement: A device is paired once, with any one existing device

The system SHALL admit a new device by pairing it out of band with a single
existing device, which appends a `device_add` event. Every other device SHALL
learn of the new member by folding the log. The system SHALL NOT require
pairing each device with each other device.

#### Scenario: A third device
- **WHEN** a third device is paired with the laptop only
- **AND** the desktop later syncs with the laptop
- **THEN** the desktop SHALL admit the third device without a further gesture

### Requirement: Admission is visible

The system SHALL surface a `device_add` for a key this device neither paired nor
emitted as a condition needing attention (§4.5 tier 1), acknowledgeable but not
silently dismissed. This is possible because `device_add` is an ordinary event:
a device admitted without the user's knowledge is a fact in the log rather than
a state nobody can see.

#### Scenario: An unexpected admission
- **WHEN** a `device_add` arrives for a key this device did not pair and did not
  itself emit
- **THEN** it SHALL be reported as needing attention
- **AND** SHALL remain reported until acknowledged

#### Scenario: An expected admission
- **WHEN** this device performed the pairing itself
- **THEN** the resulting `device_add` SHALL NOT be reported as unexpected

### Requirement: Pairing resists an attacker on the same network

The pairing gesture SHALL require something an attacker on the LAN does not
have. Proximity on the network SHALL NOT be sufficient to join a roster.

#### Scenario: A device on the network that was not invited
- **WHEN** an uninvited device attempts to pair
- **THEN** pairing SHALL fail without the out-of-band secret
