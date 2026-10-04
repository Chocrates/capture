## ADDED Requirements

### Requirement: Purging a note removes the context it was captured in

Purging SHALL delete the note's ambient row — the application, window title,
working directory, repository and branch recorded at capture — with the other
derived rows it removes.

§1.1's argument for purging the search index applies unchanged: destroying the
body while leaving a derived copy of the same material is the stated purpose
failing. A window title is frequently the same material as the note beside it.

#### Scenario: The context goes with the note
- **WHEN** a note is purged
- **THEN** no ambient row remains for it

#### Scenario: A destroyed note is not returned by a context query
- **WHEN** a note captured in a repository is destroyed
- **AND** notes are filtered by that repository
- **THEN** the destroyed note is not among them

#### Scenario: Other notes keep their context
- **WHEN** one note is purged
- **THEN** the ambient rows of other notes are untouched
