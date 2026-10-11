## MODIFIED Requirements

### Requirement: The notifier is a decision, and the default is a rule that is never written

`time`, `recurring`, `calendar_start` and `device_wake` cues SHALL fire on the
device the user has designated, recorded as a `setting` event. Until the user
chooses, a compiled-in default rule SHALL be evaluated locally and SHALL NOT be
written to the log on the user's behalf.

The default rule SHALL select exactly one device from the roster: members in
order of when each first joined, most recent first, skipping any member that
has reported it cannot show notifications and any other member this device has
not reached within a grace window. The first remaining member SHALL speak. If
none remains, this device SHALL speak.

#### Scenario: A default is not reported as a decision
- **WHEN** no notifier has been chosen
- **THEN** a default rule selects one locally
- **AND** no `setting` event exists claiming the user chose it

#### Scenario: A choice converges
- **WHEN** the user designates a notifier
- **THEN** it is recorded as a `setting` and reaches every device

#### Scenario: Two paired devices do not both speak
- **WHEN** a desktop and a phone are paired, the phone joined second, both have
  reached each other recently, and nobody has chosen a notifier
- **THEN** the phone speaks and the desktop does not

#### Scenario: A silent device hands over
- **WHEN** the most recently paired device reports that it cannot show
  notifications
- **THEN** the next device in the order speaks

#### Scenario: A device that has gone quiet hands over
- **WHEN** the most recently paired device has not been reached for longer than
  the grace window
- **THEN** a device that cannot reach it speaks itself rather than staying silent
