## ADDED Requirements

### Requirement: A mute can be lifted from the command line

`capture unmute <target>` SHALL lift a mute of that scope, where `<target>` is a
note's name, `#tag`, `@person` or `^project` — the same four forms `capture
mute` accepts.

Lifting SHALL be expressed as an ordinary `MUTE` event whose end is the present
instant. Nothing SHALL be removed from the log, and no new event kind SHALL be
introduced: mutes are applied in timestamp order with the newest one touching a
note winning, and an elapsed end already reads as not muted.

Unmuting SHALL be refused when the target is not muted, rather than appending an
event that changes nothing.

#### Scenario: A muted note is unmuted
- **WHEN** a note is muted and then unmuted
- **THEN** it SHALL appear in search again
- **AND** it SHALL appear on every entity page naming it
- **AND** the event log SHALL have grown rather than shrunk

#### Scenario: A tag mute is lifted without untagging
- **WHEN** everything carrying `#noise` is muted, and `#noise` is then unmuted
- **THEN** notes carrying the tag SHALL surface again
- **AND** they SHALL still carry the tag

#### Scenario: An entity mute is lifted by any spelling that reaches it
- **WHEN** `@sarah-chen` is muted, and `@sarah` — a spelling merged into the
  same entity — is unmuted
- **THEN** notes naming either spelling SHALL surface again

#### Scenario: Unmuting a note that is not muted is refused
- **WHEN** `capture unmute` names a note carrying no mute
- **THEN** the command SHALL exit non-zero and say the note is not muted
- **AND** no event SHALL be appended

#### Scenario: A coarser scope is accepted without knowing whether it was muted
- **WHEN** `capture unmute #noise` is run and no tag mute is in force
- **THEN** the command SHALL succeed
- **AND** it SHALL NOT claim to have lifted a mute that did not exist

Whether a *tag*, *entity* or *date range* is currently muted is not a question
the projection can answer: a mute of those scopes is recorded by its effect on
each note it covers, and nothing distinguishes a note suppressed by a tag mute
from one suppressed by its own. Refusing here would require inventing that
record, so these scopes SHALL be accepted and SHALL report what was written
rather than what changed.

### Requirement: A mute can be given an end

`capture mute <target> until <YYYY-MM-DD>` SHALL suppress the target until the
start of that day in the local zone, after which it SHALL surface again with no
further action.

A mute written with no end SHALL remain indefinite, which is the existing
behaviour and the default.

An end that has already passed SHALL be refused, because it is indistinguishable
in effect from `unmute` and reads as a mistake rather than an intent.

#### Scenario: A mute lapses on its own
- **WHEN** a note is muted until a date, and that date arrives
- **THEN** the note SHALL surface again without anybody unmuting it

#### Scenario: A dated mute holds until then
- **WHEN** a note is muted until a future date
- **THEN** it SHALL NOT appear in search before that date

#### Scenario: A date already past is refused
- **WHEN** `capture mute <note> until <a date in the past>` is run
- **THEN** the command SHALL exit non-zero
- **AND** it SHALL name `capture unmute` as what was meant

### Requirement: A mute lapses everywhere or nowhere

Every surface that suppresses a muted note SHALL treat a mute whose end has
passed as not in force. A mute SHALL NOT lapse from one surface while holding on
another.

This SHALL include review, which §2.6 names first: "mute removes a note from
review, resurfacing, and all pushed surfaces, permanently or until a stated
date."

#### Scenario: A lapsed mute restores a note to review
- **WHEN** a note is muted until a date, and that date passes
- **THEN** the note SHALL be eligible for review again

#### Scenario: An unmuted note is eligible for review
- **WHEN** a muted note is unmuted
- **THEN** the note SHALL be eligible for review again

#### Scenario: A lapsed mute restores a note to the tag index
- **WHEN** a note carrying a tag is muted until a date, and that date passes
- **THEN** the tag's count SHALL include it again
