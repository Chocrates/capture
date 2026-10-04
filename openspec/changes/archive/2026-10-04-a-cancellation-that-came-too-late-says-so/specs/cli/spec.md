## ADDED Requirements

### Requirement: A cancellation that came too late is reported

A cancellation folded after its target was destroyed SHALL be reported as
wanting attention, naming the note.

Somebody who cancels a redaction believes the content survived. On the device
that cancelled it may have; on this one it did not. Being wrong about that
silently is the failure §1.1 is guarding against when it asks for this to be
reported rather than discarded.

The report SHALL NOT offer a recovery it cannot perform.

#### Scenario: Reported after a late cancellation
- **WHEN** a cancellation has arrived after its target was destroyed
- **THEN** the status surface reports it, naming the note

#### Scenario: Nothing late, nothing said
- **WHEN** no cancellation has arrived late
- **THEN** nothing is reported about one
