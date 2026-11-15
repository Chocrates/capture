## ADDED Requirements

### Requirement: The capture chord works on macOS without a privacy grant

On macOS the overlay SHALL register the capture chord through a mechanism that
needs neither Accessibility nor Input Monitoring permission. The default chord
SHALL be ⌥Space. Registration SHALL be confirmed by one press, and a failure to
register SHALL be reported, pointing to the command line.

#### Scenario: First registration
- **WHEN** `capture-overlay --register` is run on a Mac and ⌥Space is pressed
- **THEN** it prints Confirmed
- **AND** no permission dialog was shown

#### Scenario: The chord is taken
- **WHEN** the chord is already held by another application
- **THEN** registration fails with a message naming the chord and the CLI fallback

### Requirement: The overlay is raised and focused on the chord

On macOS the overlay SHALL have no Dock icon, and on the chord SHALL come to
the front with its text field focused, and hide after submit or Escape.

#### Scenario: Capture from another app
- **WHEN** another application is focused and the chord is pressed
- **THEN** the capture field is in front and typing goes into it

### Requirement: Alerts are delivered as macOS notifications

On macOS a due alert SHALL be delivered as a system notification. A notification
that could not be sent SHALL NOT be marked delivered.

#### Scenario: osascript is blocked
- **WHEN** sending a notification fails
- **THEN** the alert stays undelivered and is retried on the next tick

### Requirement: Nothing starts at login unless asked

Installing on macOS SHALL print, not write, the login items for the overlay and
the sync daemon unless the install is run with the option that asks for them.
With that option it SHALL write LaunchAgents and load them.

#### Scenario: Plain install
- **WHEN** `make install` runs on a Mac
- **THEN** no file is written under `~/Library/LaunchAgents`
- **AND** the commands to set login start are printed

### Requirement: Sync is not installed on macOS unless asked

On macOS the sync daemon SHALL NOT be installed or started unless the install
asks for it. A peer SHALL be addable by address without discovery.

#### Scenario: Default install sends nothing
- **WHEN** a Mac is installed without the sync option
- **THEN** no process listens on the network and no data leaves the machine

#### Scenario: Manual peer
- **WHEN** `capture peers add 192.168.1.20:7878` is run
- **THEN** the next sync round tries that address

### Requirement: The health check can be read aloud

`capture doctor` SHALL print one line per check as a fixed code, a result, and
a fixed reason phrase. Its output SHALL NOT contain note text, file paths, user
or host names, addresses or keys.

#### Scenario: Sentinels never appear
- **WHEN** doctor runs against a store whose notes, path, peers and host contain known sentinel strings
- **THEN** no sentinel appears in its output

#### Scenario: A blocked notifier
- **WHEN** `osascript` is missing or refused
- **THEN** doctor prints `N1 fail` or `N2 fail` with a fixed reason
