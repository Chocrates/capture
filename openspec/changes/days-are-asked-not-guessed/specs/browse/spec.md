## ADDED Requirements

### Requirement: An unclear day is asked on both of its days

A capture with an unclear day SHALL appear on the calendar on each of its two
candidate days, with the question and both answers, and on its note page and in
review. Answering SHALL rewrite the day in the text as an ordinary edit, after
which it promotes from the text. A capture whose promotion a person has pinned
SHALL NOT be asked about.

#### Scenario: Answered from the calendar
- **WHEN** "therapy next thursday at 4pm" is answered with the later day
- **THEN** its text reads "therapy thursday oct 22 at 4pm"
- **AND** the calendar shows it on 22 October at 16:00 only

### Requirement: A suggestion names what accepting it leaves out

A suggestion SHALL name the weekday of any day it offers, say "all day" where it
offers no time, and name any clock in the text that its reading did not use.

#### Scenario: A time the reading did not use
- **WHEN** a suggestion is offered for a day while the text contains "4pm"
- **THEN** it says the 4pm was not read

### Requirement: A correction can be read again

Every surface that offers a correction or a suggestion SHALL offer, on a note
whose promotion a person pinned, to read it again. Doing so SHALL write what the
current grammar reads from the text, unpinned, so that later edits re-promote.

#### Scenario: Undoing an accepted suggestion
- **WHEN** a pinned all-day event is read again after its text was corrected to "therapy Thursday 2026-10-22 at 4pm"
- **THEN** it is an event at 2026-10-22 16:00, not pinned
