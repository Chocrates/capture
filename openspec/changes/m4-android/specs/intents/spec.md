## ADDED Requirements

### Requirement: The notifier is chosen, and until it is chosen it is a rule

The designated notifier SHALL be a `setting` event authored only by an explicit user choice. Until
that choice is made, the notifier SHALL be decided by a compiled-in default rule — the most
recently paired device that can notify — evaluated locally and never written to the log.

Pairing a device SHALL NOT emit a `setting`.

§8.9 draws the line this preserves: defaults are rules, settings are decisions. A device that wrote
a preference on the user's behalf would produce something `capture status` reports as "set by you",
which could never afterwards be improved centrally because it would look like a decision somebody
made.

#### Scenario: Pairing the first phone
- **WHEN** an Android device is paired and can notify
- **THEN** no `setting` event SHALL be authored
- **AND** the notifier SHALL be resolved by the default rule
- **AND** the surface SHALL report it as a default rather than as a choice

#### Scenario: Choosing the notifier
- **WHEN** the user picks a device to notify them
- **THEN** a `setting` SHALL be authored
- **AND** it SHALL converge to every device like any other event

### Requirement: Devices announce whether they can notify

A device SHALL report whether it is able to notify — permission granted, notifications enabled,
currently reachable — so that the choice is made among devices that can actually deliver.

#### Scenario: A device that cannot notify
- **WHEN** notification permission is absent on a device
- **THEN** it SHALL report itself as unable to notify
- **AND** SHALL NOT be offered as the notifier without that being stated
