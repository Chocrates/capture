## ADDED Requirements

### Requirement: Devices find each other on the local network

The system SHALL advertise and browse `_bsp._tcp.local` over mDNS/DNS-SD. The
TXT record SHALL carry enough for a peer to decide whether a sync is needed
without opening a connection.

#### Scenario: Two devices on one network
- **WHEN** two paired devices are on the same network
- **THEN** each SHALL discover the other without configuration

#### Scenario: A peer with nothing to offer
- **WHEN** a peer's advertised digest matches what this device already holds
- **THEN** no connection SHALL be opened

### Requirement: Nothing identifying is broadcast in the clear

The advertised device identifier SHALL be a per-network rotating pseudonym
derived from the roster key, never a stable device id. The advertised version
vector digest SHALL be keyed, never plain.

Both are requirements rather than hardening. A stable identifier broadcast on
every network a device joins is a cross-network identifier linking a person's
devices to each other at every café, office and airport — to exactly the guest
phone and housemate's laptop §5.3 names as the threat model. A plain digest
changes on every append and is therefore an activity oracle: a passive observer
learns when and how often somebody captures without ever completing a handshake.

#### Scenario: An observer on the network
- **WHEN** a device that is not on the roster observes the advertisements
- **THEN** it SHALL NOT be able to correlate a device across two networks
- **AND** it SHALL NOT be able to infer capture counts from the digest

#### Scenario: A roster member
- **WHEN** a device on the roster observes the advertisements
- **THEN** it SHALL be able to recognise its peers

### Requirement: Discovery survives networks that block mDNS

The system SHALL keep last-known addresses from previous sessions and SHALL
retry them with jittered exponential backoff when mDNS yields nothing.

#### Scenario: A corporate network with multicast blocked
- **WHEN** mDNS discovery returns no peers
- **THEN** the system SHALL attempt cached addresses
- **AND** SHALL back off between attempts rather than retrying tightly
