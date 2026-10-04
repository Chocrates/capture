## ADDED Requirements

### Requirement: A destroyed series leaves no occurrences

Destroying a capture SHALL leave no row in `occurrences` for it.

An occurrence row is not only a date. It carries `title`, `location` and
`notes`, filled from the exception bodies somebody wrote about a single
occurrence of a meeting — and the purge destroys those bodies. Keeping the
expanded copy is §1.1's stated purpose failing in the way it failed for mentions,
items and the captured context: the body is gone and a derived copy of the same
material is still readable.

A destroyed series SHALL also stop being drawable as a series: nothing on a
calendar surface SHALL show an occurrence of a capture that no longer exists.

#### Scenario: A destroyed series has no occurrences
- **WHEN** a recurring capture is destroyed
- **THEN** it SHALL have no occurrence rows

#### Scenario: An edited occurrence's words go with it
- **WHEN** an occurrence of a series carried an edited title, location or notes
- **AND** the series is destroyed
- **THEN** none of those SHALL remain readable

#### Scenario: Another series is unaffected
- **WHEN** one of two recurring captures is destroyed
- **THEN** the other SHALL keep every occurrence it had

### Requirement: The guard against a surviving row is exercised against a note that has one

The test that enumerates derived tables after a destruction SHALL use a fixture
that populates every table it is meant to check, and SHALL fail if a table it
covers holds no row before the destruction.

A guard that cannot fail is worse than a missing delete. The delete is one line;
a fixture too thin to populate a table silently exempts it, and the enumeration
reports success for a table it never looked at. `occurrences` was missed exactly
this way: the fixture is a one-off dated capture, so the table was empty and the
check had nothing to find.

Emptiness SHALL therefore be a failure rather than a pass, for any table the
test claims to cover. This is what makes the next table a test failure instead
of a finding.

#### Scenario: A table the fixture never populated is reported
- **WHEN** the enumeration covers a table that held no row before the destruction
- **THEN** the test SHALL fail, naming the table as unexercised rather than clean

#### Scenario: The fixture populates the occurrence table
- **WHEN** the fixture is captured and settled
- **THEN** it SHALL have occurrence rows, one of them carrying edited text
