## ADDED Requirements

### Requirement: The window reports sync that has never run

Where the window reports peers, it SHALL first report whether a sync round has
ever run on this device, by the same rule and the same wording the other
surfaces use. A person reading the window is the one least likely to know
whether a daemon is running, and most likely to read "never reached" as a
statement about the network.

#### Scenario: The window says the daemon has not run
- **WHEN** a paired device has never completed a sync round
- **AND** the health surface is shown
- **THEN** it reports that sync has never run on this device

#### Scenario: Every surface agrees
- **WHEN** the same archive is shown in the window, on the phone and on the command line
- **THEN** all three report the same state of sync
