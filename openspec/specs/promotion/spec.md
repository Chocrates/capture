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

### Requirement: A capture is read without being asked

A capture SHALL be read by the recognizer, and promoted if it clears the floor,
without any further action by the person who wrote it. §3.1 puts parsing after
the commit so the gesture never waits; that defers the work, it does not excuse
it. The existing requirement that editing text "triggers a reparse" (§3.1.4)
presumes an owner for that work, and this names one.

The owner SHALL be a process that is already running for other reasons, so that
promotion does not depend on a scheduler, a timer unit, or a person knowing to
ask:

- where a sync daemon runs, the daemon, on every round;
- where a foreground application is the writer, that application, each time it
  comes to the foreground;
- and on demand, by an explicit operation.

Promotion SHALL NOT depend on having a peer to sync with. A single-device
archive is the ordinary case before a second device is paired, and it is the
case in which nothing else opens the store.

#### Scenario: A capture then a read, on one device, with nothing else happening
- **WHEN** a dated task is captured on a device with no peers
- **AND** the next thing the person does is ask for their tasks
- **THEN** the task appears

#### Scenario: An unrelated write is not what fixes it
- **WHEN** a capture has not yet been read
- **THEN** it is read by the owning process
- **AND** not only as a side effect of the next unrelated write

#### Scenario: A daemon with no peers still reads
- **WHEN** the sync daemon runs on an archive with an empty roster
- **THEN** each round still reads whatever needs reading
- **AND** does so before giving up on finding a peer

### Requirement: Reading is bounded and never blocks a capture

A read-everything pass SHALL be bounded per run, and SHALL NOT be reachable from
the capture path. A grammar change over a large archive SHALL spread across
several runs rather than making any one of them slow.

#### Scenario: A grammar bump does not make a round long
- **WHEN** the grammar version changes and thousands of captures need rereading
- **THEN** each run reads at most its bound
- **AND** the remainder is read on subsequent runs

#### Scenario: A capture does not wait
- **WHEN** a capture is committed
- **THEN** the gesture returns without reading anything

### Requirement: An unread capture is visible, not silent

A surface SHALL report that a capture has not yet been read, rather than
present its state as complete when it is not. This SHALL be
answerable without involving the writer, because a surface that can only report
staleness while the writer is healthy cannot report it when it matters.

#### Scenario: A read says what it does not yet know
- **WHEN** captures are outstanding and the task list is asked for
- **THEN** the answer names how many captures have not been read

#### Scenario: Reported with the writer unavailable
- **WHEN** another process holds the writer lock
- **THEN** staleness is still reported
- **AND** the read still answers from the state it has

