## ADDED Requirements

### Requirement: A moment or series can be anchored to the sun

A moment or a recurring series SHALL be storable as a sun event (sunrise or
sunset) with an offset in minutes and a zone. It SHALL NOT store coordinates.

#### Scenario: A daily wake alarm
- **WHEN** "wake up every day at sunrise" is captured
- **THEN** a daily series anchored to sunrise with offset 0 exists
- **AND** no location is written to the log

### Requirement: A sun occurrence is named by its date

Each occurrence of a sun-anchored series SHALL be identified by its local date
in the series' zone. Its identity SHALL NOT depend on location or on the
computed time.

#### Scenario: A done mark survives a new location
- **WHEN** Tuesday's sunrise occurrence is marked done, and the phone then reports a location 500 km away
- **THEN** Tuesday's occurrence is still done

### Requirement: Location is the phone's own and never synced

The phone SHALL keep its last known location, rounded to 0.01°, as device-local
state. It SHALL NOT be written to the event log or sent to a peer.

#### Scenario: Sync carries no location
- **WHEN** the phone with a stored location syncs with the desktop
- **THEN** no event or sync message contains coordinates from it

### Requirement: The phone rings a sun alarm at the computed time

With a stored location, the phone SHALL compute the occurrence's time from the
date, the location and the sun event, add the offset, and ring it as a
time-critical alarm, including during quiet hours.

#### Scenario: Sunrise in the quiet hours
- **WHEN** sunrise at the stored location is 06:41 and quiet hours run to 07:00
- **THEN** the alarm rings at 06:41

#### Scenario: Thirty minutes before sunset
- **WHEN** a series is "every day 30 min before sunset" and sunset is 17:12
- **THEN** it rings at 16:42

### Requirement: Without a location nothing is guessed

When no location has ever been stored, a sun occurrence SHALL NOT ring, and the
phone SHALL say on the alarm and in status that it cannot ring until it knows
where the phone is. A device that never has a location (the desktop) SHALL show
the occurrence by date as "at sunrise" or "at sunset", with no time.

#### Scenario: A new phone
- **WHEN** a sun series exists and the phone has no stored location
- **THEN** no alarm is armed for it
- **AND** the phone says why

#### Scenario: The desktop calendar
- **WHEN** the desktop shows a day with a sunrise occurrence
- **THEN** it shows "at sunrise" with no clock time

### Requirement: A day without the sun event is skipped and says so

An occurrence SHALL NOT ring when the sun does not rise (or set) on its date at
the stored location, and the day SHALL show that there is
no sunrise (or sunset).

#### Scenario: Polar night
- **WHEN** the stored location is 78°N in December
- **THEN** no sunrise alarm rings that day
- **AND** the day shows "no sunrise"

### Requirement: A stale location is shown

When the stored location is more than 24 hours old, the phone SHALL show its age
beside the computed time. It SHALL still ring.

#### Scenario: Three days old
- **WHEN** the last fix is three days old
- **THEN** the alarm shows its time with "where you were 3 days ago"
