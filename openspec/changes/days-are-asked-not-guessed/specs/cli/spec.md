## ADDED Requirements

### Requirement: Answering a day and reading again from the command line

`capture day NAME 1|2` SHALL answer an unclear day with the first or second
choice as `capture show` lists them. `capture reread NAME` SHALL read a note
again, replacing a pinned correction. `capture show` and `capture cal` SHALL
name an unclear day and the command that answers it.

#### Scenario: Answered by number
- **WHEN** `capture day therapy 2` is run on "therapy next thursday at 4pm" captured Tuesday 2026-10-13
- **THEN** the text reads "therapy thursday oct 22 at 4pm"
