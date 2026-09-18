## ADDED Requirements

### Requirement: Alert deadlines are drained by the due-work mechanism

The due-work queue SHALL carry the moments promotions name, alongside the
redaction deadlines it already carries, and SHALL be drained by the same
mechanism: every process startup is the floor, with a resident process making a
deadline land near its time.

A separate timer for alerts SHALL NOT be introduced. A second scheduler is a
second thing that can be silently not running, and the failure it produces —
alerts that never arrive while every surface reports them scheduled — is the
one the single queue exists to prevent.

#### Scenario: A moment that elapsed while nothing ran is found at startup
- **WHEN** an alert's moment passed while no process was running
- **THEN** it is found at the next startup
- **AND** nothing was lost because no timer was running

#### Scenario: What is due is answerable without firing it
- **WHEN** the queue is asked what is due
- **THEN** it answers with the alerts that have come due
- **AND** asking did not deliver any of them
