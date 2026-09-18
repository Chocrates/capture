## ADDED Requirements

### Requirement: A promotion carrying a time is alertable without being armed

A promotion carrying a time SHALL become alertable by that promotion alone —
an event's start, a task's due date, or a series occurrence. No second gesture
SHALL be required to make a dated capture speak.

A promotion carrying no time SHALL NOT become alertable. A task with no due
date is not overdue and never becomes so.

#### Scenario: An event speaks at its time
- **WHEN** a capture is promoted to an event starting at a named time
- **THEN** an alert is scheduled for that time
- **AND** nothing further was required of the person who captured it

#### Scenario: A task with a due date is alertable
- **WHEN** a capture is promoted to a task carrying a due date
- **THEN** an alert is scheduled for that date

#### Scenario: A task with no due date is silent
- **WHEN** a capture is promoted to a task with no due date
- **THEN** no alert is scheduled
- **AND** it does not become overdue later

#### Scenario: An occurrence speaks, and the series does not
- **WHEN** a recurring series produces occurrences
- **THEN** each occurrence is alertable at its own time
- **AND** the series itself does not alert

### Requirement: An alert fires once for the moment it names

An alert SHALL fire once for the moment it was scheduled for. A moment that has
passed SHALL NOT be re-announced.

An alert whose moment elapsed while nothing was running SHALL fire when
something next runs, and SHALL say that it is late rather than presenting
itself as current. A machine switched off through an afternoon must not pretend
the afternoon did not happen.

#### Scenario: Firing twice is firing once
- **WHEN** an alert has fired for a moment
- **AND** the same moment is evaluated again
- **THEN** nothing further is delivered

#### Scenario: A missed moment is delivered late and says so
- **WHEN** an alert's moment passed while no process was running
- **THEN** it is delivered at the next opportunity
- **AND** it is presented as having been due earlier

#### Scenario: A cancelled occurrence does not speak
- **WHEN** an occurrence has been cancelled
- **THEN** no alert fires for it

#### Scenario: A completed thing does not speak
- **WHEN** a task or occurrence has been marked done before its moment
- **THEN** no alert fires for it

### Requirement: Muting silences alerts

A muted note SHALL NOT alert, for as long as the mute stands. This SHALL be the
means by which a dated capture that was a record rather than a reminder is kept
quiet, and it SHALL be reversible from any device (§2.6).

#### Scenario: A muted capture is silent
- **WHEN** a note carrying a time is muted
- **THEN** no alert fires for it

#### Scenario: Unmuting restores it
- **WHEN** a muted note is unmuted before its moment
- **THEN** its alert fires as scheduled

### Requirement: Editing what a capture says changes what it will say

An alert SHALL follow the moment its capture currently names. Where an edit or
a correction moves that moment, the alert moves with it; where it removes the
moment, no alert SHALL fire.

#### Scenario: A corrected time moves the alert
- **WHEN** a promotion's time is corrected before the alert has fired
- **THEN** the alert is scheduled for the new time
- **AND** nothing fires at the old one

#### Scenario: Removing the time silences it
- **WHEN** a promotion's time is removed
- **THEN** no alert fires

### Requirement: What an alert says is what was captured

An alert SHALL say the capture's own text. It SHALL NOT summarise, rephrase, or
add a fact the capture did not contain (§3.1.3).

An alert for a note whose content is withheld or destroyed SHALL NOT quote it.

#### Scenario: The alert quotes the capture
- **WHEN** an alert fires for a capture reading `dentist thursday 2pm`
- **THEN** what is said includes that text

#### Scenario: A redacted note does not speak its content
- **WHEN** an alert would fire for a note whose content has been destroyed
- **THEN** nothing of its content is said
