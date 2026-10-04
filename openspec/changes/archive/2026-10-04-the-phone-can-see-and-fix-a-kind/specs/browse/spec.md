## ADDED Requirements

### Requirement: A task surface answers what is outstanding

Every first-party surface SHALL offer a view of tasks, banded into overdue, due
on or before a horizon, and open with no due date.

A task has state and occupies no time (§3.6), so the agenda cannot hold it: the
agenda is a view of time and a task has none. Without a surface of its own an
undated task is reachable only by scrolling the timeline, which is ordered by
when a thing was captured rather than by whether it is still owed.

The undated band SHALL be present rather than folded into the others. A task
with no due date is the ordinary case — the recognizer refuses to promote a
bare task verb, so most tasks arrive by a person saying so, and a person saying
"send sarah the migration doc" is not saying when.

Done tasks SHALL NOT occupy the bands. Nothing here scores completion or counts
a streak; a finished task simply stops being outstanding.

#### Scenario: An undated task is reachable
- **WHEN** a task carries no due date
- **THEN** it SHALL appear in the task surface's undated band
- **AND** SHALL NOT appear in the agenda

#### Scenario: An overdue task is separated from one merely due
- **WHEN** a task's due date is before today in the reading zone
- **THEN** it SHALL appear in the overdue band
- **AND** a task due today or later SHALL NOT

#### Scenario: Completing a task from the surface
- **WHEN** a task is marked done from the task surface
- **THEN** it SHALL leave the bands
- **AND** the change SHALL be the same `promote` write the command line makes

#### Scenario: Nothing outstanding
- **WHEN** no task is open
- **THEN** the surface SHALL say so plainly
- **AND** SHALL NOT present an empty list as an error or a failure

### Requirement: A wrong kind is correctable from the surface that shows it

Where a surface renders what a capture is, it SHALL offer to change it. A
correction SHALL be pinned, and so SHALL beat every later reparse.

The recognizer deliberately refuses a promotion it is not confident of, and
`browse` already requires that a below-floor recognition be offered rather than
applied. An offer that cannot be accepted where it is read is not an offer. The
phone sees a misparse first because the phone is where capture happens, so the
phone is the worst possible place to be told to go and find a command line.

Demoting to a plain note SHALL be available on the same terms, because a thing
wrongly promoted is as common as a thing wrongly left alone, and `Promoted::Note`
is a promotion like any other.

#### Scenario: Accepting a below-the-floor suggestion
- **WHEN** a note shows a suggested interpretation it is not confident of
- **THEN** it SHALL be possible to accept it from that surface
- **AND** accepting SHALL write a pinned correction

#### Scenario: A pinned correction survives a later sweep
- **WHEN** a kind has been corrected by a person
- **AND** the recognizer later re-parses that capture under a newer grammar
- **THEN** the corrected kind SHALL stand

#### Scenario: Saying a note is a task
- **WHEN** a plain note is named as a task
- **THEN** it SHALL become one, with or without a due date
- **AND** the captured text SHALL be unchanged

#### Scenario: Demoting to a note
- **WHEN** a promoted capture is said to be neither a task nor an event
- **THEN** it SHALL become a plain note
- **AND** the captured text SHALL be unchanged

## MODIFIED Requirements

### Requirement: Every mutation is reachable from the window

Every first-party surface SHALL offer every mutation the command line offers
against a note, and SHALL offer device management where the device is already
shown.

§8.4 makes the window the first-party surface, and §8.10 scoped it to "every
mutation" for exactly this reason: muting exists so that suppressing something
is *cheaper* than destroying it, and that gradient inverts the moment
suppression needs a shell while destruction is a button. The same argument
applies to a lost device — the moment somebody needs to retire one is not a
moment to be looking up a command.

The phone is held to the same rule. It is a first-party surface by the same
argument, and the gradient it inverts is sharper: it is where captures are made
and so where a wrong kind is first seen.

Splitting a series SHALL be offered only on a series, since it is the only
thing a rule can be split at.

#### Scenario: Changing a series from the window
- **WHEN** a note is a series
- **THEN** the page SHALL offer to change it from a date onward
- **AND** SHALL NOT offer it on a note that is not a series

#### Scenario: Retiring a device from the window
- **WHEN** a device other than this one is listed
- **THEN** it SHALL be possible to retire it
- **AND** this SHALL require the device's key to be typed, as destruction
  requires the note's name

#### Scenario: Adding a device from the window
- **WHEN** the user starts a pairing
- **THEN** the code SHALL be shown immediately
- **AND** the window SHALL remain usable while it waits, because the answer
  arrives whenever somebody walks to the other device

#### Scenario: Changing a kind from the phone
- **WHEN** a capture's kind is shown on the phone
- **THEN** changing it SHALL be reachable without a command line
