# time-model Specification

## Purpose
TBD - created by archiving change m2-types-and-review. Update Purpose after archive.
## Requirements
### Requirement: Timed events are instants carrying their zone

A timed event SHALL be stored as UTC milliseconds together with the IANA
timezone identifier it was created in. Both are required: UTC alone cannot
expand a recurrence correctly, because "07:00 daily" means 07:00 local on both
sides of a DST boundary, which is not a fixed UTC offset.

#### Scenario: A timed event records both
- **WHEN** a capture is promoted to a timed event
- **THEN** the stored fields carry a UTC instant and the IANA zone of capture
- **AND** neither may be absent

### Requirement: All-day events are floating dates and are never converted

An all-day event SHALL be stored as a plain `YYYY-MM-DD` date with no timezone
and no conversion, ever. Converting all-day events to UTC instants makes them
jump a day for readers east or west of the storing machine.

#### Scenario: A vacation is the same days everywhere
- **WHEN** an all-day event from 1–14 August is read in Tokyo and in Denver
- **THEN** both report 1–14 August
- **AND** no instant is stored for it anywhere

#### Scenario: A due date is not midnight
- **WHEN** a task is promoted with "due Friday"
- **THEN** the due date is the floating date for that Friday
- **AND** it is not stored as 00:00 UTC on that day

### Requirement: Recurrence expands in local wall time

A recurring series SHALL expand in the series' local wall time and convert the
result to instants, never the reverse.

#### Scenario: A daily alarm survives a DST boundary
- **WHEN** a series of "07:00 daily" is expanded across a DST transition
- **THEN** every occurrence is 07:00 local
- **AND** the UTC offset between them differs across the boundary

### Requirement: DST edge cases resolve by a stated rule and are recorded

Where expansion produces a local time that does not exist, the occurrence SHALL
shift forward to the first valid instant. Where it produces an ambiguous local
time, the first SHALL win. Both are RFC 5545 conventions, and the shift SHALL be
recorded on the expanded occurrence so a view can show that it happened.

#### Scenario: Spring forward
- **WHEN** an occurrence falls at 02:30 on a day that jumps 02:00 to 03:00
- **THEN** the occurrence is placed at the first valid instant after the gap
- **AND** the occurrence records that it was shifted

#### Scenario: Fall back
- **WHEN** an occurrence falls at 01:30 on a day where 01:30 happens twice
- **THEN** the first of the two is used
- **AND** the occurrence records that the time was ambiguous

#### Scenario: A shift is never silent
- **WHEN** a shifted occurrence is displayed
- **THEN** the view marks it as shifted rather than showing it as an ordinary occurrence

### Requirement: The time crate cannot reach a clock

`capture-time` SHALL take the current instant as a value at its boundary and SHALL
NOT read a clock. A crate that can consult a clock can make the same log fold
differently on two machines, and the convergence property tests cannot observe
that.

#### Scenario: Expansion is a pure function of its inputs
- **WHEN** the same series and the same "now" are expanded twice on different machines
- **THEN** the resulting occurrences are identical

