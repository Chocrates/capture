## ADDED Requirements

### Requirement: A surface reporting peers first reports whether sync has run

A surface SHALL report whether a sync round has ever run on this device, and
SHALL do so before describing any peer. A round that ran and reached nobody and
a round that never ran are different situations with different remedies — the
first is a network to change, the second is a daemon to start — and the peer
rows describe them identically.

Where no round has ever run and the device has at least one paired peer, this
SHALL be reported as wanting attention, and SHALL name the daemon rather than
describing the peer as unreachable.

Where nothing is paired, nothing SHALL be reported. A device with no peers is
not failing to sync, and saying so on every single-device archive is how a
reader learns to skip the section.

#### Scenario: Paired, and the daemon has never run
- **WHEN** a device has a paired peer and no round has ever run
- **AND** the status surface is asked
- **THEN** it reports that sync has never run on this device
- **AND** it does not describe the peer as merely unreached

#### Scenario: Rounds run and reach nobody
- **WHEN** rounds have run recently and no peer has been reached
- **THEN** the unreached peer is reported
- **AND** the report does not claim sync has not run

#### Scenario: One device, nothing paired
- **WHEN** no peer is paired
- **THEN** nothing is said about sync having run

#### Scenario: Answerable without the writer
- **WHEN** another process holds the writer lock
- **THEN** the state of sync is still reported

### Requirement: The command line names only commands it has

An error or suggestion SHALL NOT name a command that does not exist. Being told
to run something that answers "is not a command" is worse than being told
nothing, because it ends the search in the wrong place.

#### Scenario: A refusal points at a real command
- **WHEN** a command refuses and names where to find what it needs
- **THEN** the command it names exists
