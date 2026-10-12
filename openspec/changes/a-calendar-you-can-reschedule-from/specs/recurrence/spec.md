## ADDED Requirements

### Requirement: A moved occurrence says where it came from

A surface drawing a moved occurrence SHALL draw it at its new time and SHALL
name its original start, so that a Thursday gym session reads as Monday's
moved rather than as a second series.

#### Scenario: Monday's gym, moved to Thursday
- **WHEN** the Monday 2026-10-12 14:00 occurrence of a weekly series is moved to Thursday 2026-10-15 10:00
- **THEN** Thursday shows it at 10:00, labelled as moved from Monday 12 October 14:00
- **AND** Monday does not show it at 14:00
- **AND** the following Monday's occurrence is unchanged

### Requirement: Moving to a day without a time keeps the time of day

Moving an occurrence to text naming a day and no time SHALL keep the
occurrence's original local time of day. A bare hour SHALL be refused with the
same question as at capture. The duration SHALL be kept.

#### Scenario: "thursday" means the same slot on Thursday
- **WHEN** a 14:00–15:00 occurrence is moved to "thursday"
- **THEN** it occurs on that Thursday from 14:00 to 15:00
