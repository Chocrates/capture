## ADDED Requirements

### Requirement: The confirmation refuses anything but the exact name

The prompt before a destruction SHALL proceed only when the note's name is typed
exactly, and SHALL destroy nothing otherwise.

§1.1 makes this the one confirmation that must not be trained away. A prompt
that accepted a near miss, an empty line, or the name with something after it
would be a prompt that asks for a name and means "press return" — which is worse
than no prompt, because it looks like a guard while training the reflex that
gets past it.

Declining SHALL NOT read as a failure. Choosing not to destroy something is a
legitimate outcome and the ordinary one when the prompt has done its job.

#### Scenario: A near miss destroys nothing
- **WHEN** something close to the name is typed
- **THEN** nothing SHALL be destroyed
- **AND** the outcome SHALL NOT read as an error

#### Scenario: An empty answer destroys nothing
- **WHEN** a blank line is given
- **THEN** nothing SHALL be destroyed

#### Scenario: The exact name proceeds
- **WHEN** the note's name is typed exactly
- **THEN** the destruction SHALL be ordered

### Requirement: The confirmation names everything that will go

The prompt SHALL name every note that the destruction will take, and SHALL say
when a recording goes with it.

It can only carry §1.1's weight if it says what actually goes. Somebody who has
typed a name once has learned that the prompt means one note, so a flag that
widens what it covers SHALL widen what it says — naming each additional note
rather than reporting a count.

A recording SHALL be named because somebody who would hesitate over destroying
audio should not have to infer that it is included. This clause exists because
redaction once destroyed the text and left the recording: `blobs::destroy` was
written, tested, and called by nothing.

#### Scenario: A note with a recording says so
- **WHEN** a note with an attached recording is to be destroyed
- **THEN** the prompt SHALL say the recording goes too

#### Scenario: A note without one says only what there is
- **WHEN** a note with no recording is to be destroyed
- **THEN** the prompt SHALL NOT mention a recording

#### Scenario: Every note that will go is named
- **WHEN** a destruction is widened to the notes linking to the target
- **THEN** the prompt SHALL name each of them
