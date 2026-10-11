# intents Specification

## Purpose
TBD - created by archiving change m2-types-and-review. Update Purpose after archive.
## Requirements
### Requirement: An intent is a first-class if-then plan

An `intent` event SHALL carry a cue specification and an action. It SHALL be an
ordinary event, and abandoning it SHALL be as cheap as any other retraction.

#### Scenario: An intent is stored with its cue and action
- **WHEN** an intent is created with the cue "app_focus matches Slack"
- **THEN** the event carries both the cue and the action to take

### Requirement: Only cue types this device can observe are evaluated here

The supported cue types SHALL be `time`, `recurring`, `location`, `app_focus`,
`calendar_start`, `device_wake` and `git_branch`. A device SHALL evaluate only
those it can genuinely observe.

#### Scenario: A geofence cue is not evaluated on a desktop
- **WHEN** an intent carries a `location` cue and this device has no geofence
- **THEN** this device does not fire it
- **AND** the intent remains valid for a device that can

### Requirement: Local cues fire where they are observed

`location` SHALL fire on the phone. `app_focus` and `git_branch` SHALL fire on
the machine running the application. These SHALL NOT be redirected to a
designated notifier, because moving them destroys the point of the cue.

#### Scenario: A branch cue fires at the desk
- **WHEN** a `git_branch` cue matches on the desktop
- **THEN** the desktop fires it
- **AND** it is not sent to whichever device is the designated notifier

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

