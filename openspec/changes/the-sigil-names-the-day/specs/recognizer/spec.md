## ADDED Requirements

### Requirement: A sigil may name a day rather than a full date

A `>` marker SHALL be read where it names either a complete `YYYY-MM-DD` date or
a day reference. A day reference is a weekday or a day word drawn from the same
closed vocabulary the grammar tier reads, and it SHALL resolve by the same rule:
a weekday means that weekday on or after today, and a day word means the day it
names.

This is what the published grammar has always said — `sigil = "#" slug |
"@" slug | ">" ( iso_date | day_ref )` — and `day_ref` had no implementation, so
a `>` marker that was not exactly ten bytes was recorded in the recognition and
then dropped. The prose tier then supplied a date of its own, which is the
inference §3.1.1 says SHALL NOT even be recorded as a suggestion when a sigil
disagrees with it. `>fri` written to correct a line that says "thursday"
promoted Thursday.

Resolution SHALL go through the same functions the grammar tier uses for a bare
weekday and a bare day word. Two spellings of "which Friday is `friday`" is one
decision made twice, and the second copy is free to drift from the first.

A day reference SHALL carry the same confidence a full date does. The marker was
written, so there is nothing to be unsure about.

#### Scenario: A weekday behind the sigil
- **WHEN** a capture reads `dentist thursday 2pm >fri`
- **THEN** the promotion SHALL carry the coming Friday
- **AND** Thursday SHALL NOT be recorded, not even as a suggestion
- **AND** the recognition SHALL be fully confident

#### Scenario: A day word behind the sigil
- **WHEN** a capture reads `call the bank >tomorrow`
- **THEN** it SHALL be a task due tomorrow

#### Scenario: The day the sigil names is the day prose would have named
- **WHEN** one capture reads `>friday` and another reads `friday`, at the same moment
- **THEN** both SHALL resolve to the same date

#### Scenario: A full date behind the sigil is unaffected
- **WHEN** a capture reads `pay the invoice >2026-09-20`
- **THEN** it SHALL be a task due 2026-09-20

### Requirement: An unreadable marker is not replaced by a guess

A `>` marker naming nothing the closed vocabulary can read SHALL leave the
capture with no date, and a date inferred from the prose SHALL NOT be used in
its place. The text SHALL be kept verbatim, so nothing the person wrote is lost.

The marker is an instruction. An instruction that could not be followed is not
permission to substitute a different answer, and substituting the prose date is
the defect this requirement exists to close: it is silent, it is confident, and
it is wrong in the one direction the person was trying to prevent.

The vocabulary SHALL be matched as whole words. Matching a prefix would read
`>satellite` as Saturday, which is the approximate matching §3.1.5 forbids.

#### Scenario: A marker that names nothing readable
- **WHEN** a capture reads `review the draft thursday >asdf`
- **THEN** it SHALL carry no date
- **AND** Thursday SHALL NOT be used
- **AND** the capture's text SHALL be unchanged

#### Scenario: A word that merely begins like a weekday
- **WHEN** a capture reads `launch the >satellite`
- **THEN** it SHALL carry no date

#### Scenario: Vocabulary not yet admitted stays unreadable
- **WHEN** a capture reads `plan the offsite >q2`
- **THEN** it SHALL carry no date
- **AND** adding `q2` to the vocabulary SHALL remain a deliberate edit
