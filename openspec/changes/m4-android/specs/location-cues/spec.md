## ADDED Requirements

### Requirement: A location cue fires where it is observable

A `location` cue SHALL fire on the device that observes the geofence, regardless of which device is
the designated notifier.

Only the phone can see that you have arrived somewhere. Routing the cue to a designated notifier
would deliver "you are at the hardware shop" to a desktop at home, which destroys the entire point
of point-of-performance delivery (§2.4).

#### Scenario: The phone is not the notifier
- **WHEN** a geofence is entered and another device is the designated notifier
- **THEN** the cue SHALL fire on the phone
- **AND** SHALL NOT be relocated to the notifier

### Requirement: A cue that cannot fire says so

Where location permission has not been granted, the system SHALL NOT offer to create location cues,
and SHALL report any existing `location` cue as unfirable.

§2.4 is explicit that an intent failing invisibly is worse than no intent at all: the user believes
something is watching for them when nothing is. Silence is the failure mode to design against.

#### Scenario: Permission refused, cue already exists
- **WHEN** a `location` cue exists on a phone without location permission
- **THEN** it SHALL be reported as unable to fire
- **AND** the report SHALL persist until the permission is granted or the cue is removed

#### Scenario: Permission refused, creating a cue
- **WHEN** location permission has not been granted
- **THEN** creating a location cue SHALL NOT be offered
- **AND** the reason SHALL be stated rather than the option silently missing

### Requirement: Refusing location leaves everything else working

Declining location permission SHALL NOT degrade capture, sync, reading or any other cue type.

#### Scenario: Location declined
- **WHEN** location permission is declined
- **THEN** every other capability SHALL be unaffected
