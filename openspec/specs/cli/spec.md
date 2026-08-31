# cli Specification

## Purpose
TBD - created by archiving change m2-types-and-review. Update Purpose after archive.
## Requirements
### Requirement: The command line can read the derived types

The CLI SHALL be able to list the agenda, tasks and occurrences, and SHALL
render the structured fields alongside the captured text. Every such command
SHALL take `--json`.

#### Scenario: The agenda is available in a terminal
- **WHEN** the agenda is requested for a day
- **THEN** the timed occurrences and all-day entries for that day are listed

#### Scenario: Machine-readable output is available
- **WHEN** any of these commands is given `--json`
- **THEN** the output is JSON

### Requirement: A promotion can be corrected from the command line

The CLI SHALL be able to correct a promotion, and a correction made this way
SHALL be pinned so that no later reparse overrides it.

#### Scenario: A correction survives a reparse sweep
- **WHEN** a promotion is corrected from the command line and the backlog is later reparsed
- **THEN** the correction still governs

### Requirement: Completing an occurrence does not require the GUI

The CLI SHALL be able to complete and un-complete a single occurrence of a
recurring obligation, naming it by series and original start.

#### Scenario: An instance is completed by name
- **WHEN** a single occurrence is completed from the command line
- **THEN** only that occurrence is complete
- **AND** the series is unaffected

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
- **AND** a peer known to be alive at no reachable address SHALL be shown as
  such, rather than as never seen or at an address that cannot answer

### Requirement: A lost device can be retired from the command line

The command line SHALL be able to retire a device by key, and to undo a
lockout. Retirement SHALL require confirmation, because a device retired by
mistake stops being accepted by every other.

The remedy SHALL be reachable. A retired device cannot open a session, so the
co-signing device SHALL author the re-admission itself whenever it is still a
member; the carry-a-signature flow exists for the case where every device has
been retired and none can author anything.

#### Scenario: Retiring a device
- **WHEN** the user retires a device by the key `peers` printed
- **THEN** nothing SHALL happen without confirmation
- **AND** once confirmed the device SHALL cease to be a member
- **AND** its past events SHALL remain valid

#### Scenario: The last device cannot retire itself
- **WHEN** the only member attempts to retire itself
- **THEN** it SHALL be refused, because a roster with nobody in it can never
  admit anyone again

#### Scenario: Coming back from a lockout
- **WHEN** a device asks to be re-admitted and another device vouches for it
- **THEN** the vouching device SHALL author the re-admission if it is a member
- **AND** a device SHALL NOT be able to vouch for itself

### Requirement: A recurring series can be changed from one occurrence onward

The command line SHALL be able to split a series at a date, leaving every
occurrence before it under the old rule.

The boundary SHALL be given as a date rather than an index: "the fourth one" is
not something anybody knows about their own calendar, and it stops being true
the moment an occurrence is cancelled. The date SHALL name the first occurrence
on or after it.

The new rule SHALL be read by the same recognizer a capture uses, so a phrase
means here what it means when captured, and SHALL be validated before the store
is opened — a rule that is not a rule is wrong whatever the note's occurrences
look like.

#### Scenario: Splitting a series
- **WHEN** the user splits a series at a date with a new rule
- **THEN** occurrences before that date SHALL keep the old rule
- **AND** what follows SHALL be a new note carrying the same text
- **AND** the output SHALL say so, because two entries with one name otherwise
  read as a duplicate

#### Scenario: Something that is not a rule
- **WHEN** the replacement is not a repeating rule
- **THEN** it SHALL be refused as such, whatever the series' occurrences are

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

