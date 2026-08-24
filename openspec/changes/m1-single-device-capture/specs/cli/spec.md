## ADDED Requirements

### Requirement: Capture is explicitly marked and steals no words

The system SHALL require an explicit marker to distinguish capture from subcommands, and SHALL NOT
treat unrecognised leading words as capture text.

If bare words meant capture, every subcommand name would become a phrase that could not be
captured, with no way for the user to know which words were unavailable. Silently capturing a
mistyped subcommand is worse still.

#### Scenario: Marked capture

- **WHEN** the capture marker is given followed by text
- **THEN** the text is captured and the resulting note name is printed.

#### Scenario: Unrecognised subcommand

- **WHEN** a bare word is given that matches no subcommand
- **THEN** the command reports the error and suggests the capture form, and captures nothing.

### Requirement: Capture text survives shell interpretation

The system SHALL provide a capture path in which the user's text reaches the application unaltered,
without shell word-splitting, expansion, substitution, or truncation, and SHALL NOT require the
user to quote their input.

Every sigil the system recognises is also a shell metacharacter, so a naive command alias silently
truncates at a comment character, redirects at an angle bracket, expands globs against the current
directory, interpolates environment variables, and executes command substitution. Requiring quoting
is the same failure in a politer form, because the user must still notice.

#### Scenario: Comment character does not truncate

- **WHEN** the user captures text containing a hash-prefixed tag
- **THEN** the whole line is stored, including everything after the hash.

#### Scenario: Redirection character is text

- **WHEN** the user captures text containing an angle bracket followed by a date
- **THEN** no file is created and the text is stored intact.

#### Scenario: No expansion or execution

- **WHEN** captured text contains glob characters, a variable reference, or a command substitution
- **THEN** the literal characters are stored, nothing is expanded, and nothing is executed.

#### Scenario: Explicit literal form for scripts

- **WHEN** a script uses the argument form with an end-of-options separator
- **THEN** the remainder is captured literally.

### Requirement: Input source is unambiguous

The system SHALL distinguish piped input, interactive invocation, explicit text arguments, and
empty input, and SHALL never store an empty note.

#### Scenario: Piped input

- **WHEN** text is piped to the capture command
- **THEN** the piped content is captured.

#### Scenario: Interactive invocation with no input

- **WHEN** the capture command is run interactively with no text and no pipe
- **THEN** an editor is opened.

#### Scenario: Empty result

- **WHEN** any capture path yields no content
- **THEN** nothing is stored and the command says so.

### Requirement: The CLI can drive every state the system can reach

The system SHALL provide commands to read notes, search, and inspect links; to edit, tag, alias,
pin, mute, and resolve divergences; and to redact and cancel redactions.

Several capabilities promise that an operation works from any device; a device with only a terminal
is such a device, and an integration harness cannot exercise a state it has no command to create.

#### Scenario: Correction from the terminal

- **WHEN** a note requires correction and only a terminal is available
- **THEN** a command exists to make that correction.

#### Scenario: Divergence resolution from the terminal

- **WHEN** a divergence exists
- **THEN** a command exists to inspect and dismiss it.

### Requirement: Read commands emit structured output on request

The system SHALL provide machine-readable output for every read command, so integration tests
assert against structured data rather than parsing display text.

#### Scenario: Structured output is available

- **WHEN** a read command is invoked with the structured-output flag
- **THEN** it emits parseable structured data covering the same information as the display form.

### Requirement: Operational commands expose the log directly

The system SHALL provide commands to show the raw events behind a note, to force a projection
rebuild, to report status, to list quarantined and rejected events, and to run integrity
verification.

Every fold defect reduces to "which events produced this state", so direct access to that is the
highest-value diagnostic the system can offer.

#### Scenario: Raw events for a note

- **WHEN** the user requests the events behind a note
- **THEN** every event targeting it is listed in order with its kind and origin.

#### Scenario: Forced rebuild

- **WHEN** the user requests a projection rebuild
- **THEN** the projection is rebuilt from the log and the duration is reported.

### Requirement: Export and restore are invocable

The system SHALL provide commands to run an export and to restore from exported files, including
the non-default mode that ignores redactions issued after a stated time.

The export capability is otherwise a feature nothing can trigger.

#### Scenario: Export runs on demand

- **WHEN** the export command is invoked
- **THEN** events not previously exported are written to the export directory and the count is
  reported.

#### Scenario: Restore is invocable

- **WHEN** the restore command is invoked against export files
- **THEN** the events are ingested, a new incarnation is minted, and the result is reported.

#### Scenario: Deliberate recovery mode is explicit

- **WHEN** restore is invoked with the ignore-redactions-after option and a timestamp
- **THEN** only redactions at or before that time are applied, and the command states that it is
  operating in that mode.

### Requirement: Destructive commands keep their safeguards under automation

The system SHALL confirm before redacting, and SHALL provide a flag that suppresses only the
confirmation prompt. The scope cap, the ingest rate limit, the cancellation window, and the
pre-redaction snapshot SHALL all remain in force when that flag is used. A bulk cancellation
command SHALL exist.

A flag that removes the safeguards along with the prompt turns a scripted loop into the exact
unbounded destruction the safeguards exist to bound.

#### Scenario: Confirmation by default

- **WHEN** a redaction is requested interactively
- **THEN** the user is shown what will be destroyed and must confirm.

#### Scenario: Automation retains limits

- **WHEN** redaction is invoked with the prompt suppressed
- **THEN** the scope cap, rate limit, cancellation window and snapshot still apply.

#### Scenario: Bulk cancellation

- **WHEN** many redactions have been issued in error within the cancellation window
- **THEN** a single command cancels them.

### Requirement: Exit codes distinguish outcome classes

The system SHALL return distinct exit codes for success, usage error, not-found, and integrity
failure. Verification SHALL treat the presence of rejected events as an expected condition reported
in its summary rather than as an integrity failure.

#### Scenario: Distinct codes

- **WHEN** a command fails because an argument is malformed versus because a note does not exist
- **THEN** the two exit codes differ.

#### Scenario: Rejected events do not fail verification

- **WHEN** verification runs on a store containing rejected events but no corrupted rows
- **THEN** it reports them and exits zero.
