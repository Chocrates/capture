## ADDED Requirements

### Requirement: A carried-out destruction leaves the queue

Purging a note SHALL remove its entry from the queue of pending destructions.

The fold already declines to queue a note whose body is gone, so a rebuilt
projection has no such entry. A purge edits the projection in place, so what the
fold would no longer write has to be removed where the purge happens or it stays
for ever.

#### Scenario: The queue empties as destructions are carried out
- **WHEN** a queued destruction is carried out
- **THEN** it is no longer queued

#### Scenario: A second drain finds nothing to do
- **WHEN** a drain has carried out every queued destruction
- **AND** another drain runs
- **THEN** it reports nothing destroyed

#### Scenario: Other pending destructions are untouched
- **WHEN** one of several queued destructions is carried out
- **THEN** the others are still queued

### Requirement: Cancelling a destruction already carried out is refused

Cancelling SHALL be refused once the destruction has happened, and SHALL say
that it may already have been carried out.

While the queue kept its row, a cancellation found it, wrote the event, and
reported that nothing had been destroyed. The content was gone. Being told a
recovery happened when it did not is worse than being told nothing, in the one
operation that cannot be walked back.

#### Scenario: Cancelling after the destruction
- **WHEN** a destruction has been carried out
- **AND** its cancellation is attempted
- **THEN** it is refused
- **AND** nothing claims the content was preserved

#### Scenario: Cancelling inside the window still works
- **WHEN** a destruction is still queued
- **THEN** cancelling it succeeds
