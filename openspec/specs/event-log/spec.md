# event-log Specification

## Purpose
TBD - created by archiving change m2-types-and-review. Update Purpose after archive.
## Requirements
### Requirement: The log gains the type, plan, review and occurrence kinds

The event log SHALL accept `promote`, `intent`, `review`, `occ_move`,
`occ_edit`, `occ_cancel`, `occ_done` and `occ_undone`. Each SHALL have a golden
encoding vector written before anything writes one, because the encoding is
frozen from its first write.

#### Scenario: A new kind round-trips byte for byte
- **WHEN** an event of each new kind is encoded and decoded
- **THEN** the result is identical to the input
- **AND** its id recomputes to the same value

#### Scenario: An older build holds rather than misreads
- **WHEN** a build that does not recognise one of these kinds ingests it
- **THEN** the event is held with a recorded reason
- **AND** nothing is discarded

### Requirement: Occurrence completion is idempotent and reversible

Two `occ_done` events for the same occurrence SHALL be equivalent to one. An
`occ_undone` SHALL be able to supersede a completion by HLC.

#### Scenario: Ticking twice is ticking once
- **WHEN** the same occurrence is completed twice
- **THEN** it is complete once

#### Scenario: A mistaken tick can be undone
- **WHEN** an `occ_undone` follows an `occ_done` for the same occurrence
- **THEN** the occurrence is not complete

### Requirement: A move beats a cancel on the same occurrence

Where `occ_cancel` and `occ_move` name the same occurrence, the move SHALL win
and the cancel SHALL be retained without applying. A resurrected occurrence is
visible and one gesture to dismiss; a wrongly cancelled one is invisible and
silently missed.

#### Scenario: A moved occurrence is not hidden by a concurrent cancel
- **WHEN** one device cancels an occurrence and another moves it
- **THEN** the occurrence appears at its new time
- **AND** the cancellation remains in the log

### Requirement: Promotion conflicts resolve by authority before recency

Among promotions of equal authority the later by HLC SHALL win. A human-authored
promotion SHALL beat a parser-authored one regardless of HLC.

#### Scenario: Authority outranks the clock
- **WHEN** a parser promotion has a later HLC than a human promotion for the same note
- **THEN** the human promotion governs

### Requirement: Events from a peer enter through the same door as local ones

The system SHALL ingest events received from a peer by the same path that
appends local ones: signature verification, `bhash` checking, chain and sequence
rules, and the quarantine rules for unknown kinds SHALL apply unchanged.

There SHALL NOT be a second ingest path optimised for batches. Two ways into the
log is two sets of rules about what may enter it, and the second set is the one
nobody remembers to update.

#### Scenario: A malformed event from a peer
- **WHEN** an event arrives over the wire whose `bhash` does not match its body
- **THEN** it SHALL be rejected exactly as a local one would be
- **AND** SHALL be reported the same way

#### Scenario: An unknown kind from a newer peer
- **WHEN** an event arrives with a kind this build does not know
- **THEN** it SHALL be held rather than discarded
- **AND** SHALL be re-evaluated after an upgrade

### Requirement: An origin's stream tolerates arriving out of order

An origin's events SHALL be gap-free in `seq`, and the system SHALL hold events
that arrive ahead of their predecessors rather than folding a stream with a hole
in it or rejecting the event outright.

#### Scenario: A range arrives out of order
- **WHEN** `desktop:41520` arrives before `desktop:41519`
- **THEN** the fold SHALL NOT advance past 41518 until 41519 arrives
- **AND** 41520 SHALL be retained rather than requested again

#### Scenario: The same event arrives twice
- **WHEN** an event already held arrives again from another peer
- **THEN** it SHALL be ignored
- **AND** the fold SHALL be unchanged

### Requirement: A peer cannot write to another origin's stream

The system SHALL reject an event whose signature does not match the origin it
claims, whatever peer relayed it. Relaying SHALL NOT confer authority.

#### Scenario: A relayed event with a bad signature
- **WHEN** a peer relays an event signed by a key that is not the claimed
  origin's
- **THEN** it SHALL be rejected
- **AND** SHALL be reported as a rejected event (§4.5 tier 1)

### Requirement: The log gains the entity rename, merge and bind kinds

The event log SHALL accept `entity_rename`, `entity_merge` and `entity_bind`.
Each SHALL have a golden encoding vector written before anything writes one,
because the encoding is frozen from its first write.

Each SHALL name an entity by its `ent_id` rather than by a spelling, so that a
later rename cannot orphan an event that was about the entity rather than about
the word.

None of the three SHALL be inferred. An entity is created by a note naming it,
and everything else here is somebody deciding.

#### Scenario: A new kind round-trips byte for byte
- **WHEN** an event of each new kind is encoded and decoded
- **THEN** the result is identical to the input
- **AND** its id recomputes to the same value

#### Scenario: An older build holds rather than misreads
- **WHEN** a build that does not recognise one of these kinds ingests it
- **THEN** the event is held with a recorded reason
- **AND** nothing is discarded
- **AND** the event is re-evaluated after the build is upgraded

#### Scenario: An entity event is not about a note
- **WHEN** an entity event is folded
- **THEN** it SHALL apply without requiring a note to exist as its target

### Requirement: Merging is idempotent and order-independent

Two `entity_merge` events joining the same pair SHALL be equivalent to one. Two
devices merging the same pair while partitioned SHALL converge to one answer
without either being rejected.

#### Scenario: The same merge twice
- **WHEN** the same pair is merged twice
- **THEN** the result is the same as merging once

#### Scenario: Both devices merged the same pair
- **WHEN** two devices each merge A into B and then sync
- **THEN** both devices agree which entity survives

### Requirement: Item completion names a position

`item_done` and `item_undone` SHALL identify the item by its position in the
capture they target. They SHALL keep their kind numbers, and the body SHALL
retain the item's text as written so that a log dump remains legible.

An event written before this names its item only by text. Such an event SHALL be
applied where that text still matches an item of the capture, and SHALL be held
with a recorded reason where it does not — never silently dropped, and never
applied to a different item because the words happen to match it now.

#### Scenario: A completion survives its words changing
- **WHEN** an item is ticked and the capture is later edited to reword it
- **THEN** the completion SHALL still apply to that item

#### Scenario: An older event whose text still matches
- **WHEN** an event written before this change names an item that still exists
- **THEN** it SHALL apply to that item

#### Scenario: An older event whose text no longer matches
- **WHEN** an event written before this change names text no item now has
- **THEN** it SHALL be held with a recorded reason
- **AND** SHALL NOT be applied to any other item

