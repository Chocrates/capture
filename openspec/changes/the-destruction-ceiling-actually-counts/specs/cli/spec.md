## ADDED Requirements

### Requirement: Held redactions are reported and can be released

A held redaction SHALL be reported as wanting attention, naming how many are
waiting, and SHALL be releasable so that destruction proceeds.

Holding is only a remedy if somebody can see it and act on it. A ceiling that
stops a runaway and says nothing leaves an archive in a state no surface
explains, and §1.1 says explicitly that what is over the ceiling is held
*pending explicit confirmation* — which requires something to confirm with.

#### Scenario: Something is being held
- **WHEN** redactions have been held by the ceiling
- **THEN** the status surface reports how many, as wanting attention

#### Scenario: Releasing one
- **WHEN** a held redaction is released
- **THEN** its destruction proceeds on the next drain

#### Scenario: Nothing held says nothing
- **WHEN** nothing is being held
- **THEN** nothing is reported about the ceiling
