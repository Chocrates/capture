## ADDED Requirements

### Requirement: Alerts are subject to the notification budget

Every alert SHALL pass through the budget of §2.6 before it is delivered:
quiet hours, batching, and cross-device deduplication.

**Quiet hours** SHALL be on by default and SHALL hold an alert until they end,
except for a cue the user armed explicitly for a moment inside them. A person
who set an alarm for 6am meant 6am.

**Batching** SHALL be the default regime rather than an overflow behaviour.
A time-critical alert — a moment that was named, such as a meeting starting or
an armed alarm — SHALL be delivered immediately. Everything else SHALL join the
next delivery.

**Deduplication** SHALL prevent two devices delivering the same alert within a
short window, so that §2.4.1's accepted duplicate costs one extra buzz rather
than an unbounded number.

#### Scenario: A named moment is immediate
- **WHEN** an event starting at a named time comes due
- **THEN** it is delivered at that time rather than held for a batch

#### Scenario: A due date joins the batch
- **WHEN** a task carrying a due date but no time of day comes due
- **THEN** it is delivered with the next batch rather than at midnight

#### Scenario: Quiet hours hold an alert
- **WHEN** an alert comes due inside quiet hours
- **AND** it is not a moment the user armed
- **THEN** it is delivered when quiet hours end

#### Scenario: An armed alarm overrides quiet hours
- **WHEN** the user armed a cue for a moment inside quiet hours
- **THEN** it is delivered at that moment

#### Scenario: Two devices deliver once
- **WHEN** two devices would deliver the same alert within the dedup window
- **THEN** it is delivered once

### Requirement: A phone fires time alerts

A phone SHALL schedule and deliver alerts for the moments its archive names,
rather than relying on another device being awake. Time alerts SHALL survive
the application not running.

This is not a redirection of the notifier rule (§2.4.1), which decides *which*
device speaks where more than one could. It is the requirement that the phone
is capable of speaking at all.

#### Scenario: An alert survives the app being closed
- **WHEN** an alert is scheduled
- **AND** the application is not running when its moment arrives
- **THEN** it is still delivered

#### Scenario: The phone does not wait for the desktop
- **WHEN** a phone holds a capture naming a moment
- **AND** no other device is running
- **THEN** the phone delivers the alert itself
