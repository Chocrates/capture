## ADDED Requirements

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
