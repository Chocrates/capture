## ADDED Requirements

### Requirement: A sigil names the date and a verb names the kind

A task verb at the head of a capture SHALL compose with a date named by a sigil,
producing a task due on that date rather than an event occupying it.

The sigil rule is unchanged: where an explicit sigil and an inference disagree
about *which date*, the sigil wins. That rule answers a different question from
this one. §3.6 separates a thing that occupies time from a thing that is owed,
and the verb is what says which — so reading the sigil and ignoring the verb
answers only half of what was written.

This matters most where it is least expected: the more precisely somebody writes
a date, the more certainly they land on the wrong half of the table.

#### Scenario: A verb and a sigil date
- **WHEN** a capture reads `pay the invoice >2026-09-20`
- **THEN** it SHALL be a task due 2026-09-20
- **AND** SHALL NOT be an event

#### Scenario: A sigil date with no verb is unchanged
- **WHEN** a capture reads `dentist >2026-09-20`
- **THEN** it SHALL be an event on that day

#### Scenario: The sigil still decides which date
- **WHEN** a capture names one date with a sigil and another in prose
- **THEN** the sigil's date SHALL be the one used

#### Scenario: Confidence is unchanged
- **WHEN** a date is named by a sigil
- **THEN** the recognition SHALL remain fully confident

### Requirement: A date written in full is read wherever it appears

A complete date written as `YYYY-MM-DD` SHALL be recognised in a capture's text,
not only behind a sigil.

An unambiguous date that the archive cannot see is the worst of both: nothing is
guessed, and nothing is recorded either. It composes with a task verb like any
other date.

#### Scenario: A bare date in text
- **WHEN** a capture reads `dentist 2026-09-20`
- **THEN** it SHALL be recognised as that day

#### Scenario: A bare date with a verb
- **WHEN** a capture reads `pay the invoice on 2026-09-20`
- **THEN** it SHALL be a task due that day

#### Scenario: Something that is not a date is not read as one
- **WHEN** a capture contains `2026-13-45`
- **THEN** no date SHALL be recognised from it

### Requirement: A month name is read either way round

A day and a month name SHALL be recognised in either order: `sep 20` and
`20 sep` SHALL name the same day.

Day-first is how most of the world writes a date, and reading only one order
means the form silently works for some people and not others.

#### Scenario: Day before month
- **WHEN** a capture reads `pay the invoice on 20 sep`
- **THEN** it SHALL be a task due on the twentieth of September

#### Scenario: Month before day
- **WHEN** a capture reads `pay the invoice on sep 20`
- **THEN** it SHALL name the same day as the form above

#### Scenario: A number that is not a day
- **WHEN** a capture reads `pay 40 sep` with a number no month has
- **THEN** no date SHALL be recognised from it

### Requirement: An ambiguous numeric date is not guessed at

A date written only in digits and separators SHALL NOT be read as a date, such
as `20/09` or `20.09.2026`.

`09/20` and `20/09` are the same characters meaning different days, and nothing
in the capture says which convention was meant. §3.1.5 rates a match that fires
wrongly as worse than no match at all, and this is the case where being wrong is
invisible — a date off by months, recorded confidently.

#### Scenario: A slashed date names nothing
- **WHEN** a capture reads `pay the invoice on 20/09`
- **THEN** no date SHALL be recognised

#### Scenario: A dotted date names nothing
- **WHEN** a capture reads `pay the invoice on 20.09.2026`
- **THEN** no date SHALL be recognised
