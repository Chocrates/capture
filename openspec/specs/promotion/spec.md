# promotion Specification

## Purpose
TBD - created by archiving change m2-types-and-review. Update Purpose after archive.
## Requirements
### Requirement: Promotion annotates and never rewrites the capture

A `promote` event SHALL assign a derived type and its fields without rewriting,
consuming or relocating the captured text. The text SHALL remain verbatim
forever.

#### Scenario: A wrong parse leaves a whole note
- **WHEN** a capture is promoted with the wrong date
- **THEN** the captured text is unchanged
- **AND** the note is intact and merely wrongly labelled

#### Scenario: Nothing is moved out of the note stream
- **WHEN** a capture is promoted to a calendar event
- **THEN** the note still appears in the timeline

### Requirement: Promotion is reversible and a later promotion supersedes

`promote` SHALL be an ordinary event that a later one supersedes, and a
correction SHALL be possible from any device at any time.

#### Scenario: A misparse is corrected later
- **WHEN** a note promoted to the wrong Thursday is corrected
- **THEN** the correction is a new event
- **AND** the original promotion remains in the log

### Requirement: A human correction is never overridden by a reparse

A `promote` authored by a person SHALL carry `pinned: true` and SHALL beat a
parser-authored promotion regardless of HLC ordering. Plain last-write-wins is
insufficient here because the parser will improve and reparse the backlog.

#### Scenario: A later machine parse loses to an earlier human one
- **WHEN** a reparse produces a promotion for a note a person has already corrected
- **THEN** the human promotion still governs
- **AND** the machine promotion is retained in the log without effect

#### Scenario: Machine promotions among themselves are last-write-wins
- **WHEN** two parser-authored promotions exist for one note
- **THEN** the later by HLC governs

### Requirement: A note carries text, fields and a descriptor

A promoted note SHALL carry the captured text, the current structured fields,
and a regenerated one-line descriptor of those fields. The text SHALL be the
episodic record and the fields SHALL be operationally authoritative; neither
SHALL be regenerated from the other.

#### Scenario: Drift is visible rather than silent
- **WHEN** the text says "every day at 7" and the fields say weekdays
- **THEN** both are shown
- **AND** neither is rewritten to match the other

#### Scenario: Editing text reparses; editing fields does not rewrite text
- **WHEN** the captured text is edited
- **THEN** a reparse is triggered, respecting pinned corrections
- **WHEN** the fields are edited through a form
- **THEN** only the fields change

### Requirement: A task is a promotion with state and an optional floating due date

A task SHALL be a `promote` to `as: "task"` carrying open/done state, an optional
due date stored as a floating date, optional list membership and optional
ordering. Lists SHALL be tags with a display order so a task may sit in several
without duplication.

#### Scenario: A due date is a date
- **WHEN** a task is due Friday
- **THEN** the due date is a floating date rather than an instant

#### Scenario: A task belongs to several lists at once
- **WHEN** a task is placed in two lists
- **THEN** one task exists, carrying both

### Requirement: Recurring obligations do not accumulate unless told to

Uncompleted instances of a recurring obligation SHALL be recorded as lapsed and
SHALL NOT roll over, stack or accumulate. A per-series `carries_forward` flag
SHALL default to false and, when set, SHALL keep uncompleted instances visible
until completed or dismissed. The recognizer SHALL NOT set it.

#### Scenario: A missed walk creates no debt
- **WHEN** Tuesday's instance of a daily walk is not completed
- **THEN** Tuesday's instance is recorded as lapsed
- **AND** Wednesday shows one instance, not two

#### Scenario: A missed dose does carry
- **WHEN** a series with `carries_forward` set has an uncompleted instance
- **THEN** it remains visible until completed or explicitly dismissed

#### Scenario: The flag is never inferred
- **WHEN** the recognizer promotes a recurring obligation
- **THEN** `carries_forward` is false
- **AND** a series that has lapsed repeatedly prompts once, in review, to ask

