## ADDED Requirements

### Requirement: One implementation of every rule

The Kotlin side SHALL render and handle lifecycle. It SHALL NOT decide anything the core already
decides: no conflict resolution, no slug generation, no recurrence expansion, no timezone
arithmetic, no confidence scoring, no fold.

The conflict rules were property-tested across two milestones on the assumption that there is one
implementation of them. A second implementation in another language is not a duplicate of the
first — it is the one nobody exercises, and it diverges silently because both sides look correct
in isolation.

#### Scenario: A rule reimplemented on the Kotlin side
- **WHEN** the Kotlin sources are examined for date arithmetic, conflict resolution, slug
  generation or recurrence expansion
- **THEN** none SHALL be present
- **AND** this SHALL be checked mechanically rather than by review

### Requirement: The binding surface is operation-shaped

The FFI SHALL expose operations — capture, read a note, search, list a timeline, pair, sync —
rather than the store's types and primitives.

A fine-grained surface invites the caller to assemble behaviour out of parts, and the first thing
assembled that way is something the fold already decides. A coarse surface makes that awkward.

#### Scenario: Capturing through the binding
- **WHEN** the phone captures text
- **THEN** the binding SHALL accept the text and return the note's permanent name
- **AND** the caller SHALL NOT have to construct an event, a nonce, or a timestamp

### Requirement: A capture is durable before the call returns

The binding SHALL commit a capture to the log before returning, and SHALL NOT queue it for later
writing on the Kotlin side.

§2.1's promise is that a capture the interface accepted is never lost. A Quick Settings tile can be
destroyed the moment its callback returns, so a queue in Kotlin moves the durability boundary into
the layer with the most lifecycle hazards and the least testing.

#### Scenario: The surface is destroyed immediately after capturing
- **WHEN** a capture returns and the calling component is destroyed
- **THEN** the capture SHALL be in the log
- **AND** SHALL survive a process kill with no further action
