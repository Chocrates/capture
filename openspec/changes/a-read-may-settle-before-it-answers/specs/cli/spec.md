## MODIFIED Requirements

### Requirement: A read reports what it has not read

A read command whose answer depends on promotion SHALL report any outstanding
captures alongside its answer, and SHALL name the remedy.

Reporting staleness SHALL be done from the read-only handle: a read SHALL NOT
take the writer lock in order to *discover* that it is behind (§8.8).

A read whose answer is meaningless while it is behind MAY take the writer lock
in order to *resolve* it, and SHALL report rather than fail when it cannot. The
distinction is between the two uses of the lock, not between two kinds of
command: discovering staleness never needs it, and removing staleness is the
only thing that can.

`capture due` is that case, and the reason is worth keeping because the
alternative shipped. Reading the question weakly — open a session, drop the
result — meant a store somebody else was writing gave no sweep, no error, and an
answer from stale state. "Nothing is due" is exactly the answer a person is
there to disbelieve, and it is the one they act on by doing nothing.

The report is one line and SHALL NOT suppress the answer. An empty task list
with "3 captures not yet read" is a different statement from an empty task list,
and the difference is the one a person needs.

#### Scenario: An empty list that is not necessarily empty
- **WHEN** a dated task was captured and not yet read
- **AND** the task list is requested
- **THEN** the list is shown as it stands
- **AND** a line reports the outstanding captures and names the command that reads them

#### Scenario: Nothing outstanding, nothing said
- **WHEN** every capture has been read
- **THEN** no staleness line appears

#### Scenario: A read does not become a write
- **WHEN** a read command runs while the writer lock is held
- **THEN** it answers
- **AND** reports the outstanding captures

#### Scenario: A read that must not answer stale settles first
- **WHEN** `capture due` runs and captures are unread
- **THEN** it SHALL read them before answering

#### Scenario: And says so when it cannot
- **WHEN** `capture due` runs while another process holds the writer lock
- **THEN** it SHALL answer from what it has
- **AND** SHALL say that it could not read the outstanding captures
