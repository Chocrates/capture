## ADDED Requirements

### Requirement: The timeline is grouped by capture day

The system SHALL present notes grouped by the local day they were captured in, using the timezone
recorded at capture, ordered within a day by capture time.

Grouping by day gives temporal recall something stable to work against; using the viewing device's
timezone would misfile notes made while travelling, which is when situational cues matter most.

#### Scenario: Days are grouped and ordered

- **WHEN** the timeline is opened
- **THEN** notes appear under day headings, most recent day first.

#### Scenario: Travel does not misfile notes

- **WHEN** a note captured late at night in one timezone is viewed on a device in another
- **THEN** it appears under the day it was captured in.

### Requirement: Machine activity never reorders the timeline

The system SHALL move a note within the timeline only in response to a human action. Enrichment and
any automated reprocessing SHALL update a note in place.

A view that reshuffles on a schedule the user did not set destroys the spatial memory they would
otherwise use to find things.

#### Scenario: Human edit bumps the note

- **WHEN** the user edits a note
- **THEN** it moves to the top of its own day.

#### Scenario: Automated update does not move it

- **WHEN** an automated process updates a note
- **THEN** its position is unchanged.

### Requirement: Every note state renders rather than being an edge case

The system SHALL render notes that are incomplete, redacted, diverged, or muted with an explicit
indication of that state, and SHALL NOT render any note as blank or omit it silently.

#### Scenario: Incomplete note

- **WHEN** a note has no readable text yet
- **THEN** it renders with its kind and status rather than as an empty row.

#### Scenario: Redacted note

- **WHEN** a note has been redacted
- **THEN** it renders as redacted with the date.

#### Scenario: Diverged note

- **WHEN** a note has competing versions
- **THEN** it renders with a marker that expands to show both, without blocking or prompting.

#### Scenario: Muted note

- **WHEN** a note is muted
- **THEN** it remains visible in the timeline with an indication that it is suppressed elsewhere.

### Requirement: Ambient context is visible and filters the view

The system SHALL display the ambient context captured with each note, and SHALL allow any context
element to filter the timeline to notes sharing it.

Context is frequently the recognition cue when the text is not, which is the reason it is collected
at all; hiding it behind an interaction wastes it.

#### Scenario: Context is shown by default

- **WHEN** a note has recorded context
- **THEN** that context is visible on the row without further interaction.

#### Scenario: Filtering by context

- **WHEN** the user selects a context element
- **THEN** the timeline narrows to notes sharing it.

### Requirement: Capture is not available as an on-screen control

The system SHALL NOT provide an on-screen button that creates a note. Capture SHALL be the same
gesture everywhere.

An on-screen control becomes the path of least resistance, after which the gesture is no longer
uniform across contexts.

#### Scenario: No new-note button

- **WHEN** the timeline is displayed
- **THEN** no control exists whose purpose is to begin a new note.

#### Scenario: Degraded capture is signposted

- **WHEN** the capture gesture is unavailable on this device
- **THEN** the empty state names the working alternative rather than advertising a gesture that does nothing.

### Requirement: Every action is reachable from the graphical surface

The system SHALL make every operation on a note available from the graphical interface — editing,
tagging, aliasing, pinning, muting, resolving divergences, and redacting — not only from the command
line.

A surface you can read but not act on pushes ordinary gestures into a second tool. Muting the note
in front of you is the clearest case: muting exists so that suppressing something is *cheaper* than
destroying it, and that gradient inverts if suppression requires a terminal while destruction is a
documented command.

#### Scenario: Mute from the note in view

- **WHEN** a note is open and the user mutes it
- **THEN** it is muted without leaving the interface.

#### Scenario: Divergence resolved in place

- **WHEN** a diverged note is open
- **THEN** both versions are visible and the divergence can be dismissed there.

### Requirement: Destructive confirmation resists reflex dismissal

The system SHALL, when confirming a redaction in the graphical interface, enumerate exactly what
will be destroyed and SHALL require a deliberate act rather than a default-focused accept button.

This is the only destructive confirmation in the system and it must not be trained away by
appearing routinely. A modal dialog is materially more dismissible than a terminal prompt, because
dismissing dialogs is a reflex — so the graphical confirmation has to work harder than the terminal
one to convey the same weight.

#### Scenario: Confirmation names what is destroyed

- **WHEN** a redaction is confirmed in the interface
- **THEN** the affected notes are listed individually rather than summarised as a count alone.

#### Scenario: Accept is not the default action

- **WHEN** the confirmation appears
- **THEN** pressing Enter or Escape does not destroy anything.

### Requirement: A pending redaction is visible and cancellable where it is seen

The system SHALL display a live countdown for any queued redaction, with a cancel that actually
cancels it.

A cancellation window that requires finding a terminal is not much of a safety net, and this is the
only live element the interface has.

#### Scenario: Countdown is visible

- **WHEN** a redaction is queued
- **THEN** the interface shows the remaining time until destruction.

#### Scenario: Cancel works from the interface

- **WHEN** the user cancels a pending redaction from the interface
- **THEN** the destruction is aborted and the content remains readable.

#### Scenario: Countdown disappears when the window closes

- **WHEN** the window elapses and destruction occurs
- **THEN** the countdown is gone rather than lingering as a dead control.

### Requirement: Search returns matching notes ranked by relevance

The system SHALL provide full-text search over note text, titles and transcripts, returning
matching notes ranked by relevance, and SHALL exclude content that has been redacted.

Retrieval is the faculty this design leans on, so search is not a secondary surface.

#### Scenario: Search finds a note by its content

- **WHEN** a term appearing in a note is searched
- **THEN** that note is returned.

#### Scenario: Results are ranked

- **WHEN** several notes match
- **THEN** they are ordered by relevance rather than arbitrarily.

#### Scenario: Redacted content is unfindable

- **WHEN** a term that appeared only in a redacted note is searched
- **THEN** nothing is returned.

### Requirement: Nothing in the system scores the user

The system SHALL NOT display a streak, a completion percentage, an accuracy figure, a capture-count
target, or any aggregate that characterises the user's performance — at any level, on any surface.

Counts describing the *data* are fine; figures describing the *person* are not. The distinction is
not cosmetic here: repeated performance failure is a documented mechanism behind the anxiety this
design is built for, so a surface that manufactures a running failure signal works directly against
the reason the system exists.

#### Scenario: No performance aggregate is displayed

- **WHEN** any view is rendered
- **THEN** it contains no streak, score, accuracy or completion percentage describing the user.

#### Scenario: Data counts are permitted

- **WHEN** a view reports how many notes exist or how many events are quarantined
- **THEN** that is a description of the data and is allowed.

### Requirement: Attention-demanding notifications are bounded

The system SHALL deliver at most one notification per day for conditions requiring attention, SHALL
never place their content on a lock screen, and SHALL deliver immediately and without counting
against that budget only those conditions that degrade with time — a full capture spool being the
case in M1.

An unbounded stream of system-health interruptions competes for exactly the attention this design
is trying not to consume.

#### Scenario: Routine condition is rate-limited

- **WHEN** several attention-requiring conditions arise in one day
- **THEN** at most one notification is delivered.

#### Scenario: Time-critical condition is immediate

- **WHEN** the capture spool is filling
- **THEN** notification is immediate, because the failure grows with every capture rather than with
  the calendar.

#### Scenario: Content stays off the lock screen

- **WHEN** a notification is delivered while the device is locked
- **THEN** it names the condition without exposing note content.

### Requirement: Notes are addressable by readable name

The system SHALL address notes by their canonical slug or an assigned alias, in a single shared
namespace, so no alias can shadow another note's canonical name.

#### Scenario: Note is reachable by slug

- **WHEN** a slug is given to a command or link
- **THEN** the corresponding note is resolved.

#### Scenario: Alias cannot shadow a slug

- **WHEN** an alias is assigned that matches another note's canonical slug
- **THEN** the assignment is refused.

### Requirement: Links record what they meant when written

The system SHALL record, for each explicit link, both the name as written and the note it resolved
to at authoring time, and SHALL render a link as broken when the two disagree rather than resolving
it to the new target.

Silently repointing an old reference at different content is the specific failure a memory system
cannot afford.

#### Scenario: Alias reassignment breaks rather than retargets

- **WHEN** an alias is moved to a different note
- **THEN** existing links written against it render as broken, showing both names.

#### Scenario: Backlinks are listed

- **WHEN** a note page is opened
- **THEN** every note linking to it is listed.

#### Scenario: Link to redacted target

- **WHEN** a link's target has been redacted
- **THEN** the link states that rather than silently disappearing.

### Requirement: Suggested relationships are kept separate from asserted ones

The system SHALL present machine-suggested relationships in a distinct section from links the user
wrote, and SHALL allow promotion of a suggestion to an explicit link without triggering
reinterpretation or reordering of the note.

Merging suggested with asserted leaves the user unable to trust either.

#### Scenario: Suggestions are visually distinct

- **WHEN** a note has both written links and suggested relationships
- **THEN** they appear in separate sections.

#### Scenario: Promotion has no side effects

- **WHEN** a suggestion is promoted to an explicit link
- **THEN** the note is not reinterpreted and does not move in the timeline.

### Requirement: Health reporting is tiered and honestly worded

The system SHALL classify operational conditions into three tiers: conditions requiring user action,
conditions worth knowing, and diagnostics. In this milestone tier one SHALL include a full capture
spool, rejected events, a position collision, a capture gesture that has stopped working, and
required export pruning; tier one items SHALL be acknowledgeable rather than permanently present.
Conditions SHALL be described by their actual cause rather than by a single generic label.

Peer-related conditions — an unreachable replica, an undeliverable redaction — are deferred with
the sync capability, because no peers exist yet and a surface built against permanently empty data
would ship untested.

A dashboard of undifferentiated warnings trains the user to ignore it, and most of these conditions
are the system working correctly rather than errors.

#### Scenario: Expected conditions are not alarming

- **WHEN** events are quarantined because they come from a newer client
- **THEN** they are described as such and are not presented as errors.

#### Scenario: Authentication failures are not buried

- **WHEN** an event fails authentication
- **THEN** it is reported at tier one, not as a diagnostic.

#### Scenario: Unresolvable items can be acknowledged

- **WHEN** a tier-one item cannot be resolved by the system, such as pruning an external backup
- **THEN** the user can acknowledge it and it stops demanding attention while remaining visible.

### Requirement: Health reporting states the healthy case and never overclaims

The system SHALL state affirmatively when nothing is wrong, rather than showing an empty surface,
and SHALL NOT report as verified anything it has not actually checked.

Absence of information is ambiguous in a way an explicit statement is not — and the ambiguity is
itself a cost this system would otherwise generate for free. But an unqualified positive about
something unverified is worse than the ambiguity, in a system whose purpose is compensating for
unreliable memory.

#### Scenario: Healthy state is stated explicitly

- **WHEN** nothing is wrong
- **THEN** the status says so affirmatively rather than being blank.

#### Scenario: Unverified state is not reported as verified

- **WHEN** a check has not run, or could not complete
- **THEN** the status reports that it has not run, rather than reporting success.
