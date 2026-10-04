## ADDED Requirements

### Requirement: An export applies the redactions the device knows about

An export SHALL write without their bodies the events of any note a
non-cancelled redaction names, whether or not the destruction has yet been
carried out.

Redaction hides a note immediately and destroys its body after a window. An
export written inside that window would carry the full text of a note already
ordered destroyed into a file created after the order — a file redaction can
never reach afterwards.

The events SHALL be kept in place and emptied rather than omitted, as the
credential strip already does: dropping them would leave a hole in an origin's
numbering, which this design treats as unrecoverable.

#### Scenario: A redaction ordered but not yet carried out
- **WHEN** a note has been redacted and the destruction has not happened
- **AND** an export is written
- **THEN** that note's events are written without their bodies

#### Scenario: A cancelled redaction does not strip
- **WHEN** a redaction has been cancelled
- **THEN** the note's events are exported with their bodies

#### Scenario: Ordinary notes are unaffected
- **WHEN** no redaction names a note
- **THEN** its events are exported whole

### Requirement: What an export withheld is reported by reason

An export SHALL say what it wrote without bodies, distinguishing content
withheld because it looks like a credential from content withheld because a
redaction names it.

The two call for different responses: a credential flag may be a false positive
to clear, and a redaction is a decision somebody made that nothing should undo.
Reporting them as one number would invite clearing the wrong thing.

#### Scenario: Both kinds in one export
- **WHEN** an export withholds both a credential-flagged note and a redacted one
- **THEN** each is reported with its own reason

#### Scenario: Only redactions
- **WHEN** an export withholds only redacted notes
- **THEN** nothing suggests clearing a credential flag
