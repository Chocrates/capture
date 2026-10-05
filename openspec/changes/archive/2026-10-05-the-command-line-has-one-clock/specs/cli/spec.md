## ADDED Requirements

### Requirement: The command line reads one clock

Every command SHALL take the current time from one place, and that place SHALL
be movable in a build made for testing and fixed in a build made for use.

Time decides irreversible things here. Whether a destruction has passed its
cancellation window, whether a copy has outlived its retention, whether a task
is overdue — each is a comparison against "now", and each was reachable only at
whatever moment the suite happened to run. So the commands that do the most
dangerous work were the least exercised: `capture recover` is invoked by no test
at all, because producing a snapshot means waiting out a thirty-minute window.

A moved clock SHALL NOT be possible in a build somebody installs. A faked path
sends a command at the wrong archive and is visible in what it answers; a faked
clock **stamps events**, and an archive whose ordering came from an environment
variable is a damaged one that looks fine.

#### Scenario: Commands agree about what time it is
- **WHEN** two commands run in the same moment
- **THEN** both SHALL have taken the time from the same place

#### Scenario: A test can move time
- **WHEN** a build made for testing is told the time
- **THEN** commands SHALL use it

#### Scenario: An installed build cannot
- **WHEN** a build made for use is told the time
- **THEN** it SHALL be ignored, and the real clock used
