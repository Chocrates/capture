# cli Specification

## Purpose
TBD - created by archiving change m2-types-and-review. Update Purpose after archive.
## Requirements
### Requirement: The command line can read the derived types

The CLI SHALL be able to list the agenda, tasks and occurrences, and SHALL
render the structured fields alongside the captured text. Every such command
SHALL take `--json`.

Where any capture has not yet been read, these commands SHALL report that
alongside their output, so that an answer derived from incomplete promotion is
never presented as a complete one.

#### Scenario: The agenda is available in a terminal
- **WHEN** the agenda is requested for a day
- **THEN** the timed occurrences and all-day entries for that day are listed

#### Scenario: Machine-readable output is available
- **WHEN** any of these commands is given `--json`
- **THEN** the output is JSON

#### Scenario: An incomplete answer says that it is incomplete
- **WHEN** captures are outstanding and any of these commands is run
- **THEN** the output reports how many captures have not been read

### Requirement: A promotion can be corrected from the command line

The CLI SHALL be able to correct a promotion, and a correction made this way
SHALL be pinned so that no later reparse overrides it.

#### Scenario: A correction survives a reparse sweep
- **WHEN** a promotion is corrected from the command line and the backlog is later reparsed
- **THEN** the correction still governs

### Requirement: Completing an occurrence does not require the GUI

The CLI SHALL be able to complete and un-complete a single occurrence of a
recurring obligation, naming it by series and original start.

#### Scenario: An instance is completed by name
- **WHEN** a single occurrence is completed from the command line
- **THEN** only that occurrence is complete
- **AND** the series is unaffected

### Requirement: Pairing and peer state are visible from the command line

The command line SHALL be able to pair a device, list peers with when each was
last reached, and report what is outstanding against each. Every read SHALL
take `--json`.

#### Scenario: Pairing a second device
- **WHEN** the user runs the pairing command on both devices
- **THEN** a `device_add` SHALL be appended
- **AND** both devices SHALL report the other as a peer

#### Scenario: Asking about peers
- **WHEN** the user asks for peers
- **THEN** each SHALL be listed with when it was last reached
- **AND** a peer known to be alive at no reachable address SHALL be shown as
  such, rather than as never seen or at an address that cannot answer

### Requirement: A lost device can be retired from the command line

The command line SHALL be able to retire a device by key, and to undo a
lockout. Retirement SHALL require confirmation, because a device retired by
mistake stops being accepted by every other.

The remedy SHALL be reachable. A retired device cannot open a session, so the
co-signing device SHALL author the re-admission itself whenever it is still a
member; the carry-a-signature flow exists for the case where every device has
been retired and none can author anything.

#### Scenario: Retiring a device
- **WHEN** the user retires a device by the key `peers` printed
- **THEN** nothing SHALL happen without confirmation
- **AND** once confirmed the device SHALL cease to be a member
- **AND** its past events SHALL remain valid

#### Scenario: The last device cannot retire itself
- **WHEN** the only member attempts to retire itself
- **THEN** it SHALL be refused, because a roster with nobody in it can never
  admit anyone again

#### Scenario: Coming back from a lockout
- **WHEN** a device asks to be re-admitted and another device vouches for it
- **THEN** the vouching device SHALL author the re-admission if it is a member
- **AND** a device SHALL NOT be able to vouch for itself

### Requirement: A recurring series can be changed from one occurrence onward

The command line SHALL be able to split a series at a date, leaving every
occurrence before it under the old rule.

The boundary SHALL be given as a date rather than an index: "the fourth one" is
not something anybody knows about their own calendar, and it stops being true
the moment an occurrence is cancelled. The date SHALL name the first occurrence
on or after it.

The new rule SHALL be read by the same recognizer a capture uses, so a phrase
means here what it means when captured, and SHALL be validated before the store
is opened — a rule that is not a rule is wrong whatever the note's occurrences
look like.

#### Scenario: Splitting a series
- **WHEN** the user splits a series at a date with a new rule
- **THEN** occurrences before that date SHALL keep the old rule
- **AND** what follows SHALL be a new note carrying the same text
- **AND** the output SHALL say so, because two entries with one name otherwise
  read as a duplicate

#### Scenario: Something that is not a rule
- **WHEN** the replacement is not a repeating rule
- **THEN** it SHALL be refused as such, whatever the series' occurrences are

### Requirement: A peer that has not been reached is reported loudly

A peer not reached in over a week SHALL be reported as needing attention
(§4.5 tier 1). It is the leading indicator of the failure this milestone exists
to prevent: a phone that has silently stopped syncing holds captures that exist
on exactly one device.

#### Scenario: A phone that stopped syncing
- **WHEN** a paired peer has not been reached for over a week
- **THEN** the status surface SHALL report it as needing attention
- **AND** SHALL say when it was last reached rather than only that it is stale

#### Scenario: A peer reached this morning
- **WHEN** a peer was reached within the week
- **THEN** it SHALL NOT be reported as needing attention

### Requirement: Reading outstanding captures can be asked for

The CLI SHALL provide a command that reads whatever captures have not been read
and reports how many were promoted. It is a write command and SHALL take the
writer lock like any other; it exists so that a person on a machine with no
daemon has a remedy to be pointed at, and so that "nothing was read" can be
distinguished from "it was read and found nothing".

#### Scenario: Outstanding captures are read on request
- **WHEN** captures have not been read and the command is run
- **THEN** they are read
- **AND** the number promoted is reported

#### Scenario: Nothing to do says so
- **WHEN** everything has already been read
- **THEN** the command reports that nothing was outstanding
- **AND** exits successfully

#### Scenario: The writer is busy
- **WHEN** another process holds the writer lock
- **THEN** the command reports that it could not take it
- **AND** does not claim to have read anything

### Requirement: A read reports what it has not read

A read command whose answer depends on promotion SHALL report any outstanding
captures alongside its answer, and SHALL name the remedy. This SHALL be done
from the read-only handle: a read SHALL NOT take the writer lock in order to
discover or to resolve staleness (§8.8).

The report is one line and SHALL NOT suppress the answer. An empty task list
with "3 captures not yet read" is a different statement from an empty task
list, and the difference is the one a person needs.

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

### Requirement: A surface reporting peers first reports whether sync has run

A surface SHALL report whether a sync round has ever run on this device, and
SHALL do so before describing any peer. A round that ran and reached nobody and
a round that never ran are different situations with different remedies — the
first is a network to change, the second is a daemon to start — and the peer
rows describe them identically.

Where no round has ever run and the device has at least one paired peer, this
SHALL be reported as wanting attention, and SHALL name the daemon rather than
describing the peer as unreachable.

Where nothing is paired, nothing SHALL be reported. A device with no peers is
not failing to sync, and saying so on every single-device archive is how a
reader learns to skip the section.

#### Scenario: Paired, and the daemon has never run
- **WHEN** a device has a paired peer and no round has ever run
- **AND** the status surface is asked
- **THEN** it reports that sync has never run on this device
- **AND** it does not describe the peer as merely unreached

#### Scenario: Rounds run and reach nobody
- **WHEN** rounds have run recently and no peer has been reached
- **THEN** the unreached peer is reported
- **AND** the report does not claim sync has not run

#### Scenario: One device, nothing paired
- **WHEN** no peer is paired
- **THEN** nothing is said about sync having run

#### Scenario: Answerable without the writer
- **WHEN** another process holds the writer lock
- **THEN** the state of sync is still reported

### Requirement: The command line names only commands it has

An error or suggestion SHALL NOT name a command that does not exist. Being told
to run something that answers "is not a command" is worse than being told
nothing, because it ends the search in the wrong place.

#### Scenario: A refusal points at a real command
- **WHEN** a command refuses and names where to find what it needs
- **THEN** the command it names exists

### Requirement: One occurrence of a series can be skipped

The command line SHALL be able to cancel a single occurrence of a recurring
series, leaving the rule and every other occurrence untouched. The occurrence
SHALL be named by its original start, as the series' other exception commands
already name one.

This is the gesture for "not this Thursday". Editing the rule changes every
future occurrence, and leaving the entry standing shows a meeting that is not
happening — so without this the two available answers are both wrong.

#### Scenario: Skipping one occurrence
- **WHEN** one occurrence of a series is skipped
- **THEN** that occurrence is marked cancelled
- **AND** the series' rule is unchanged
- **AND** every other occurrence is unaffected

#### Scenario: Skipping what is already skipped
- **WHEN** an occurrence that is already cancelled is skipped again
- **THEN** it is not an error
- **AND** the occurrence is still cancelled

#### Scenario: A series that does not exist
- **WHEN** a note that is not a series is given
- **THEN** it is refused, naming what was expected

### Requirement: A skipped occurrence can be restored

The command line SHALL be able to restore a skipped occurrence. Restoring SHALL
be expressed as moving the occurrence back to its own original start, because a
move beats a cancellation and no un-cancel event exists.

That asymmetry is deliberate and SHALL NOT be removed by adding one: a
resurrected occurrence is visible and one gesture away, where a wrongly
cancelled one is invisible and silently missed.

#### Scenario: Restoring a skipped occurrence
- **WHEN** a skipped occurrence is restored
- **THEN** it appears again at its original time
- **AND** the cancellation remains in the log

#### Scenario: Restoring one that was never skipped
- **WHEN** an occurrence that is not cancelled is restored
- **THEN** it is not an error
- **AND** the occurrence still appears at its original time

### Requirement: The redaction confirmation names a recording it will destroy

Where a note being redacted carries a recording, the confirmation SHALL say so.
§1.1 makes this the one confirmation in the system that must not be trained
away, and it can only be that if it names what actually goes — a person who
would hesitate over an audio recording should not have to infer that it is
included.

#### Scenario: Redacting a note with a recording
- **WHEN** redaction is confirmed for a note carrying a blob
- **THEN** the prompt says the recording will be destroyed too

#### Scenario: Redacting a note without one
- **WHEN** the note carries no blob
- **THEN** the prompt does not mention a recording

### Requirement: A misfired redaction can be recovered while its snapshot lives

The command line SHALL be able to list the snapshots taken before redactions
and still within their retention, and SHALL be able to bring one back.

A snapshot that cannot be read is a plaintext copy of content somebody ordered
destroyed, kept for thirty days, with its cost paid and its purpose unreachable.

#### Scenario: Listing what can be recovered
- **WHEN** a redaction has been carried out and its snapshot has not expired
- **THEN** it is listed, with when it was taken and what it begins with

#### Scenario: Nothing to recover
- **WHEN** no snapshot is within its retention
- **THEN** that is said plainly rather than shown as an empty list

#### Scenario: An expired snapshot is not offered
- **WHEN** a snapshot's retention has elapsed
- **THEN** it is not listed
- **AND** recovering it is refused

### Requirement: Recovery is a new capture, and says so

Recovering SHALL write the snapshot's text as a new capture, and SHALL say that
is what happened. It SHALL NOT present the redaction as undone: the body is gone
from the log, and the redaction is retained so that it destroys matching events
arriving later.

#### Scenario: Recovering brings the text back
- **WHEN** a snapshot is recovered
- **THEN** a new note carries its text
- **AND** the output says a new note was written rather than that the redaction was reversed

#### Scenario: The redaction still stands
- **WHEN** a snapshot has been recovered
- **THEN** the original note is still redacted

### Requirement: Held redactions are reported and can be released

A held redaction SHALL be reported as wanting attention, naming how many are
waiting, and SHALL be releasable so that destruction proceeds.

Holding is only a remedy if somebody can see it and act on it. A ceiling that
stops a runaway and says nothing leaves an archive in a state no surface
explains, and §1.1 says explicitly that what is over the ceiling is held
*pending explicit confirmation* — which requires something to confirm with.

#### Scenario: Something is being held
- **WHEN** redactions have been held by the ceiling
- **THEN** the status surface reports how many, as wanting attention

#### Scenario: Releasing one
- **WHEN** a held redaction is released
- **THEN** its destruction proceeds on the next drain

#### Scenario: Nothing held says nothing
- **WHEN** nothing is being held
- **THEN** nothing is reported about the ceiling

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

### Requirement: A cancellation that came too late is reported

A cancellation folded after its target was destroyed SHALL be reported as
wanting attention, naming the note.

Somebody who cancels a redaction believes the content survived. On the device
that cancelled it may have; on this one it did not. Being wrong about that
silently is the failure §1.1 is guarding against when it asks for this to be
reported rather than discarded.

The report SHALL NOT offer a recovery it cannot perform.

#### Scenario: Reported after a late cancellation
- **WHEN** a cancellation has arrived after its target was destroyed
- **THEN** the status surface reports it, naming the note

#### Scenario: Nothing late, nothing said
- **WHEN** no cancellation has arrived late
- **THEN** nothing is reported about one

### Requirement: A redaction can take the notes linking to its target

The command line SHALL offer a way to redact a note together with every note
that links to it.

A note's slug is derived from its opening words, and a note linking to it
carries that slug inside its own verbatim text. Destroying the target
re-derives its name from its id, which stops the name carrying the words — and
leaves the copies already written into other notes, where no redaction reaches
them.

It SHALL follow links one hop only. A note linking to a note linking to the
target has copied nothing of the target, and a transitive sweep turns a
deliberate act into an accident.

#### Scenario: Redacting a note and what points at it
- **WHEN** a note is redacted with its links
- **THEN** the target is destroyed
- **AND** every note linking to it is destroyed

#### Scenario: Links are followed one hop
- **WHEN** a note links to a note that links to the target
- **THEN** that further note is not destroyed

#### Scenario: Without the flag nothing changes
- **WHEN** a note is redacted without asking for its links
- **THEN** only that note is destroyed

### Requirement: The confirmation names every note that will be destroyed

The confirmation SHALL list each note the redaction will destroy, by name.

§1.1 makes this the one confirmation in the system that must not be trained
away by appearing routinely. A flag that quietly widens what a familiar
confirmation covers is precisely how it would be — somebody who has typed the
name once learns the prompt means one note.

#### Scenario: Several notes will go
- **WHEN** a redaction with links will destroy more than the target
- **THEN** the confirmation names each of them

#### Scenario: Declining destroys nothing
- **WHEN** the confirmation is declined
- **THEN** no note is queued for destruction

### Requirement: Too many to name is refused

The command SHALL refuse, and say so, where the target and the notes linking to
it exceed the number a redaction may name — rather than redacting those that fit.

Redacting the first that fit would leave the rest holding the words the whole
operation exists to remove, while reporting success.

#### Scenario: More linking notes than a redaction may name
- **WHEN** the target and its backlinks exceed the cap
- **THEN** nothing is redacted
- **AND** the refusal says how many there were

