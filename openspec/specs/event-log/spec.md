# event-log Specification

## Purpose
TBD - created by archiving change m2-types-and-review. Update Purpose after archive.
## Requirements
### Requirement: The log gains the type, plan, review and occurrence kinds

The event log SHALL accept `promote`, `intent`, `review`, `occ_move`,
`occ_edit`, `occ_cancel`, `occ_done` and `occ_undone`. Each SHALL have a golden
encoding vector written before anything writes one, because the encoding is
frozen from its first write.

#### Scenario: A new kind round-trips byte for byte
- **WHEN** an event of each new kind is encoded and decoded
- **THEN** the result is identical to the input
- **AND** its id recomputes to the same value

#### Scenario: An older build holds rather than misreads
- **WHEN** a build that does not recognise one of these kinds ingests it
- **THEN** the event is held with a recorded reason
- **AND** nothing is discarded

### Requirement: Occurrence completion is idempotent and reversible

Two `occ_done` events for the same occurrence SHALL be equivalent to one. An
`occ_undone` SHALL be able to supersede a completion by HLC.

#### Scenario: Ticking twice is ticking once
- **WHEN** the same occurrence is completed twice
- **THEN** it is complete once

#### Scenario: A mistaken tick can be undone
- **WHEN** an `occ_undone` follows an `occ_done` for the same occurrence
- **THEN** the occurrence is not complete

### Requirement: A move beats a cancel on the same occurrence

Where `occ_cancel` and `occ_move` name the same occurrence, the move SHALL win
and the cancel SHALL be retained without applying. A resurrected occurrence is
visible and one gesture to dismiss; a wrongly cancelled one is invisible and
silently missed.

#### Scenario: A moved occurrence is not hidden by a concurrent cancel
- **WHEN** one device cancels an occurrence and another moves it
- **THEN** the occurrence appears at its new time
- **AND** the cancellation remains in the log

### Requirement: Promotion conflicts resolve by authority before recency

Among promotions of equal authority the later by HLC SHALL win. A human-authored
promotion SHALL beat a parser-authored one regardless of HLC.

#### Scenario: Authority outranks the clock
- **WHEN** a parser promotion has a later HLC than a human promotion for the same note
- **THEN** the human promotion governs

