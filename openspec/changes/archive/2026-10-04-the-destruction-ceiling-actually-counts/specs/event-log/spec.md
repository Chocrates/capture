## ADDED Requirements

### Requirement: Destruction is bounded per origin and the bound counts targets

A receiving replica SHALL bound how much one origin may destroy, by counting
the targets that origin's redactions name. The count SHALL be per origin: one
device going wrong must not spend another's budget, and a compromised key must
not be able to exhaust the allowance of a device that is behaving.

The history SHALL be read from the log, where redactions and their targets
remain available — their bodies are retained deliberately so that a redaction
goes on destroying matching events that arrive later.

#### Scenario: One origin issuing far too much is held
- **WHEN** an origin's redactions name more targets within an hour than the ceiling allows
- **THEN** the destructions beyond the ceiling are held rather than carried out
- **AND** the content of the held targets is still present

#### Scenario: Another origin is unaffected
- **WHEN** one origin is over the ceiling
- **THEN** a redaction from a different origin still proceeds

#### Scenario: Ordinary use is not held
- **WHEN** an origin redacts a number of notes well inside the ceiling
- **THEN** every one is carried out

### Requirement: The window is measured against the redaction's own time

The ceiling SHALL be measured over the hour around a redaction's own timestamp,
not the hour before the moment destruction happens to be carried out.

A runaway issues its redactions in seconds. If the window were measured from the
time of the drain, that batch arriving on a replica after a month offline would
count nothing against the ceiling and would all proceed — which is exactly the
receiving-replica case the bound exists for.

#### Scenario: A clustered batch arriving late is still held
- **WHEN** an origin's redactions were all issued within one hour
- **AND** they are drained long afterwards
- **THEN** those beyond the ceiling are still held

#### Scenario: The same number spread over time proceeds
- **WHEN** an origin's redactions are spread across many hours
- **THEN** none is held for the ceiling

### Requirement: A redaction may not name more targets than the cap

A redaction naming more than the cap SHALL be refused rather than silently
reduced to the first that fit. Dropping the remainder with no record would
report success while leaving named content intact.

#### Scenario: Too many targets
- **WHEN** a redaction naming more than the cap is folded
- **THEN** it is not treated as a valid redaction of the first that fit
