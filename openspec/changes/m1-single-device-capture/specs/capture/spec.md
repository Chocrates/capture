## ADDED Requirements

### Requirement: Capture is one gesture with no decisions

The system SHALL accept a capture through a single gesture that presents no choice of type,
destination, title, tag, or category. Commit SHALL be a single key and discard SHALL be a single
key, with no third option.

Every choice presented at capture time is a working-memory allocation at the moment capacity is
lowest, and answering wrongly is what makes material unfindable later.

#### Scenario: Overlay presents one field

- **WHEN** the capture overlay opens
- **THEN** it shows a single text field with no type selector, destination picker, or tag input.

#### Scenario: Commit and discard

- **WHEN** the user presses Enter
- **THEN** the capture is stored and the overlay closes; pressing Escape closes without storing.

### Requirement: The capture gesture is fixed for the milestone

The system SHALL fix the capture chord, the overlay layout, and the commit and discard keys at the
start of this milestone and SHALL NOT change them within it.

Context stability is among the strongest accelerators of habit formation, and habit formation takes
substantially longer than a milestone. A build that changes the capture surface repeatedly across
that window undermines the one lever with evidence behind it — so everything else may move, and
this may not.

#### Scenario: Gesture is unchanged across the milestone

- **WHEN** other parts of the system change during the milestone
- **THEN** the capture chord, commit key and discard key remain as they were on first run.

### Requirement: Capture never blocks

The system SHALL complete a capture without waiting on the network, on another device, on
enrichment, or on parsing. No capture SHALL be rejected or validated.

#### Scenario: No network available

- **WHEN** the device has no network connection
- **THEN** capture succeeds normally and returns immediately.

#### Scenario: Parsing is not a gate

- **WHEN** the text cannot be interpreted as any structured type
- **THEN** it is stored as a plain note; nothing is refused.

### Requirement: Capture degrades to a spool rather than failing

The system SHALL preallocate a spool at install time and SHALL write captures to it when the event
store cannot be appended to, including when the filesystem is full or identity is unresolved.
Sequence numbers SHALL be assigned when the spool drains, not when it fills. Captures SHALL carry
their true capture time and timezone through the spool. If the spool itself cannot accept a
capture, the system SHALL fail loudly rather than discard silently.

Assigning sequence numbers at spool time would leave a permanent gap if the drain were partial.

#### Scenario: Filesystem full

- **WHEN** the disk is full and the user captures
- **THEN** the capture is accepted into the spool, a persistent warning is shown, and nothing is lost.

#### Scenario: Spooled captures keep their capture time

- **WHEN** captures spooled during the morning are drained in the evening
- **THEN** they appear grouped under the morning, not the evening.

#### Scenario: Spool exhausted

- **WHEN** the spool is full and a further capture is attempted
- **THEN** the attempt fails visibly and the user is told, rather than the capture being dropped.

### Requirement: Capture collects an ambient envelope

The system SHALL attach, without asking, whatever context it can observe at capture time:
timestamp, device, capture timezone, working directory, git repository and branch where
applicable, and — on platforms that expose it — foreground application and window title.

The system SHALL NOT capture clipboard contents. Copying the clipboard into an append-only log is
how a credential ends up in it, which is the case redaction exists for and the one redaction cannot
reach in already-written exports.

The system SHALL treat a platform's absent context as absent, and SHALL NOT record a placeholder
value that a query could mistake for real data.

This is retrieval scaffolding: it is what makes an otherwise uninformative note recoverable weeks
later, and it costs the user nothing at the moment they can least afford a cost.

#### Scenario: Editor context is captured

- **WHEN** a capture is made while a git repository is the foreground working directory
- **THEN** the repository and branch are recorded on the note.

#### Scenario: Context is never requested

- **WHEN** any capture is made
- **THEN** the user is not prompted for context and cannot be blocked by its absence.

#### Scenario: Unavailable context is omitted

- **WHEN** a context source cannot be read on the current platform
- **THEN** that field is omitted and capture proceeds.

#### Scenario: A meaningless answer is not recorded as an answer

- **WHEN** a platform query returns a value that encodes "nothing" rather than failing — such as a
  null active-window identifier under a compositor that does not track one
- **THEN** the field is omitted rather than stored as an empty or zero value.

#### Scenario: Clipboard is never read

- **WHEN** any capture is made
- **THEN** no clipboard content is read or stored.

### Requirement: The capture surface is resident and fast

The system SHALL keep the capture surface resident with a pre-warmed window so that the interval
from gesture to editable cursor is under 150 ms in normal operation.

This target is a user-interface convention rather than a research finding, and it does not by
itself justify architectural cost; it is retained because a sluggish capture surface is one a
person avoids.

#### Scenario: Overlay opens promptly

- **WHEN** the capture gesture fires on a warm process
- **THEN** an editable cursor is available in under 150 ms.

#### Scenario: Capture binary is independent

- **WHEN** the main application is not running, crashes, or is updating
- **THEN** the capture surface still opens and stores captures.

### Requirement: Captured text is preserved verbatim forever

The system SHALL store the captured text exactly as entered and SHALL NOT rewrite, reformat,
consume, or relocate it. Later structure SHALL be additive annotation over it.

This is what makes a wrong interpretation survivable: a misparse becomes a wrong label on an
intact note rather than a lost note.

#### Scenario: Structure does not consume text

- **WHEN** a capture is later interpreted as having a date in it
- **THEN** the original text remains complete and unmodified.

#### Scenario: Whitespace and unicode survive

- **WHEN** a capture contains newlines, emoji, or non-Latin script
- **THEN** the stored text is byte-identical to what was entered.
