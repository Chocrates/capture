# sync-discovery Specification

## Purpose
TBD - created by archiving change m3-sync. Update Purpose after archive.
## Requirements
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

**The roster key is minted, not derived from the member set.** A retired device
holds the whole log, so anything computable from what it already has stays
computable by it — and the device retired *because it was stolen* would go on
recognising its former peers on every network it met them. An explicit secret
can be rotated, and the retired device cannot receive the replacement because
ingesting the retirement tears its sessions down. Minting and rotating are one
event kind, so the first key is simply the first rotation.

**Rotation is also what closes the counting leak.** A keyed digest still
changes whenever the vector changes, so keying alone hides the count and leaves
the timing. Because the advertised values also change every epoch on their own,
a change no longer implies an append.

**The network identifier SHALL be derived from the networks the device is
attached to**, by subnet prefix rather than host address: a host address
changes when DHCP reassigns one on the same network, which would rotate the
pseudonym for no reason and lose sight of every peer. Netmasks describing a
single address are excluded, since IPv6 privacy extensions mint a fresh one
every few hours.

#### Scenario: The pseudonym actually differs per network
- **WHEN** the same device advertises on two different networks
- **THEN** the advertised pseudonym SHALL differ
- **AND** this SHALL be true of the value the running daemon produces, not only
  of the function that computes it

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

