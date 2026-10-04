## ADDED Requirements

### Requirement: A redaction naming more than the cap applies to nothing

A redaction naming more targets than the cap SHALL be held rather than applied
to the targets that fit. Silently reducing it leaves the remainder intact while
the operation reports success, which is being confidently wrong in the one
operation that cannot be retried.

Held rather than discarded: the event is authentic and stays in the log, and
what it names can be seen.

#### Scenario: Too many targets
- **WHEN** a redaction names more targets than the cap
- **THEN** none of them is queued for destruction
- **AND** the redaction is recorded as held

#### Scenario: Exactly the cap is applied
- **WHEN** a redaction names exactly the cap
- **THEN** every one of them is queued

### Requirement: A redaction that cannot be read is held

A redaction whose body cannot be decoded SHALL be held rather than skipped. A
destruction order nobody can read is a thing somebody should be able to see,
and skipping it makes it indistinguishable from no order at all.

#### Scenario: An unreadable redaction
- **WHEN** a redaction's body cannot be decoded
- **THEN** it is recorded as held
