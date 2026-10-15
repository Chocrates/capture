## ADDED Requirements

### Requirement: A timed moment is warned of before it comes

A moment that names a time of day SHALL also come due a lead time before it,
said once as a warning, in addition to being said at the moment. The lead
SHALL default to 15 minutes and MAY be set to zero, which turns warnings off.

#### Scenario: Warned, then told
- **WHEN** `dentist thursday 2pm` is captured and the lead is 15 minutes
- **THEN** something is said at 13:45 that it is in 15 minutes, and again at 14:00

#### Scenario: No warning for something already underway
- **WHEN** a moment has already begun
- **THEN** no warning for it is due

#### Scenario: No warning for a day
- **WHEN** a task is due tomorrow, or an occurrence is all day
- **THEN** no warning is due for it

### Requirement: A timed alert on the phone is hard to miss

A timed alert on the phone SHALL play on the alarm stream, vibrate, and repeat
until it is dismissed or opened. A day-only alert SHALL NOT.

#### Scenario: Silent phone
- **WHEN** the phone's ringer is on vibrate and a timed moment arrives
- **THEN** the phone vibrates and the alarm sound plays at the alarm volume
