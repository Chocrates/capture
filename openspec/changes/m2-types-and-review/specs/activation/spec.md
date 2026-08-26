## ADDED Requirements

### Requirement: A context surfaces what was captured against it

When a meeting or context becomes current, the notes captured against it SHALL
be surfaced. This SHALL use the recurrence machinery already built plus one
event kind, and SHALL NOT require the user to have associated them by hand.

#### Scenario: A recurring meeting brings its notes
- **WHEN** a recurring meeting's occurrence starts
- **THEN** notes captured during previous occurrences of that meeting are surfaced

### Requirement: External calendars are read-only and live in a shadow namespace

External calendars SHALL be imported read-only into a shadow namespace, SHALL
appear in the calendar and agenda views, SHALL be available as ambient context,
and SHALL NOT be authored into the log as owned events or written back.

#### Scenario: A meeting is never authored into the log
- **WHEN** an external calendar is imported
- **THEN** its entries appear in the views
- **AND** no owned event is written for them

#### Scenario: Notes outlive the calendar
- **WHEN** an external calendar is removed or becomes unreachable
- **THEN** the notes captured against its meetings survive intact with their captured context
- **AND** the stale shadow entries are marked as stale rather than silently dropped
