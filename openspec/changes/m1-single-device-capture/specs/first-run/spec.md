## ADDED Requirements

### Requirement: Startup distinguishes four states

The system SHALL detect, at startup, whether this is a fresh installation, a normal start, a restore
in which identity was lost, or a restore in which the event store was lost or replaced. Detection
SHALL be two-sided: the identity record and the event store are each checked for consistency with
the other.

An earlier three-state model omitted "identity survived, store lost", which is reachable by a crash
during first run and by a user deleting the store to start over. In that state the identity agrees
with itself, so nothing trips, and the device then reuses sequence positions that other replicas
already hold.

#### Scenario: Fresh installation

- **WHEN** no store directory exists
- **THEN** the system performs first-run setup.

#### Scenario: Normal start

- **WHEN** identity and event store are present and consistent
- **THEN** the system starts normally without minting a new incarnation.

#### Scenario: Identity lost

- **WHEN** the event store is present and identity is missing or does not match it
- **THEN** the system treats this as a restore and mints a new incarnation before any append.

#### Scenario: Store lost or replaced

- **WHEN** identity is present but the event store is missing, empty, or holds fewer events for this
  origin than the identity record says were written
- **THEN** the system treats this as a restore and mints a new incarnation before any append.

### Requirement: The consistency check is directional

The system SHALL compare the identity record's highest written sequence against the event store and
SHALL treat only a **store that has fallen behind** as a restore. A store that is ahead of the
identity record SHALL be treated as normal, and the record updated.

The two are separate files with no atomic relationship, so an ordinary crash between committing an
event and updating the record leaves the record behind by one. Treating that symmetrically would
mint a fresh origin after every crash — fragmenting the origin history permanently and, once other
replicas exist, growing the version vector without bound.

#### Scenario: Record lags after a crash

- **WHEN** the store holds more events for this origin than the identity record notes
- **THEN** the system starts normally and updates the record; no new incarnation is minted.

#### Scenario: Store has rolled back

- **WHEN** the store holds fewer events for this origin than the identity record notes
- **THEN** the system treats it as a restore and mints a new incarnation.

### Requirement: An unresolved startup state blocks appends, never capture

The system SHALL refuse to append to the log while startup state is unresolved, and SHALL continue
to accept captures into the spool.

Refusing to write to a position that may not belong to this origin is correct; refusing to accept
the user's input is not, and would contradict the guarantee that capture always succeeds.

#### Scenario: Capture during an unresolved state

- **WHEN** the user captures while startup state is unresolved
- **THEN** the capture is spooled and acknowledged, and is sequenced once the state resolves.

#### Scenario: Resolution is automatic, not a prompt

- **WHEN** the store has fallen behind the identity record
- **THEN** the system mints a new incarnation without asking — restore is used in an emergency and
  must not stall on a dialogue.

#### Scenario: A genuinely undecidable state does ask

- **WHEN** the identity record is unreadable or internally inconsistent, so neither direction can be
  established
- **THEN** the system asks, refuses to append, and continues to spool captures.

### Requirement: First run establishes identity and a signed roster root

The system SHALL, on first run, create the store, generate a device keypair, mint a random
incarnation, preallocate the capture spool, and emit a self-signed roster root admitting itself.

A self-signed root is valid only as the first event of an origin with no predecessor in a log
containing no other root. That exception is narrow and checkable; the alternative — treating an
empty roster as implicit permission — has no such bound.

#### Scenario: Store is initialised

- **WHEN** first run completes
- **THEN** the store, keypair, incarnation and spool exist and the roster contains this device.

#### Scenario: Spool is preallocated while disk is available

- **WHEN** first run completes
- **THEN** the spool has been allocated, because its purpose is to work when the filesystem is full.

#### Scenario: A second roster root is quarantined

- **WHEN** a self-signed roster root is ingested into a log that already has one
- **THEN** it is quarantined rather than rejected — its signature verifies, so it occupies its
  position legitimately — and it does not join the roster.

### Requirement: First run writes no settings

The system SHALL treat defaults as compiled-in rules and SHALL write preference events only when the
user makes a choice.

Materialising defaults as recorded decisions makes them permanent and unimprovable, and destroys the
ability to distinguish a deliberate choice from an inherited one.

#### Scenario: No preference events on first run

- **WHEN** first run completes
- **THEN** no preference events have been written.

#### Scenario: Provenance is distinguishable

- **WHEN** status is displayed
- **THEN** each setting is shown as either a default or a user choice.

### Requirement: Hotkey registration is confirmed, not assumed

The system SHALL verify that the capture gesture actually reaches the application before treating
setup as complete, SHALL name the default chord explicitly, SHALL report conflicts with existing
bindings where the platform allows detection, and SHALL raise a tier-one condition if a registered
gesture has never fired within a defined period on an otherwise active device.

Registration succeeding is not the same as the gesture working: an exclusive grab can be taken by
another application, a system-reserved chord can be consumed before delivery, and a compositor can
accept a binding while routing the key elsewhere. Each produces a healthy-looking installation that
captures nothing — indistinguishable from disuse, and liable to be blamed on the concept.

#### Scenario: Registration is confirmed by use

- **WHEN** first run registers the capture gesture
- **THEN** the user is asked to press it once, and setup is not complete until the event arrives.

#### Scenario: Conflict is reported

- **WHEN** the default chord is already bound and the platform exposes that
- **THEN** the conflict is reported with the conflicting owner where available.

#### Scenario: Dead gesture is detected

- **WHEN** a registered gesture has not fired for the defined period while the device is otherwise in use
- **THEN** a tier-one condition is raised.

### Requirement: Failed registration degrades rather than blocking

The system SHALL continue to start when the capture gesture cannot be registered, SHALL report the
failure with the platform-specific remedy, SHALL keep it as a tier-one condition until resolved, and
SHALL present the working alternative capture path in the empty state.

Blocking startup over a gesture turns a degraded installation into no installation, while failing
silently produces an installation that appears to work and does not.

#### Scenario: Application still starts

- **WHEN** gesture registration fails
- **THEN** the application starts and the terminal capture path works.

#### Scenario: Empty state names the working path

- **WHEN** the gesture is unavailable and no notes exist
- **THEN** the empty state names the working alternative rather than the gesture.

### Requirement: Nothing is offered before it is needed

The system SHALL NOT require an account, a network connection, or pairing to complete first run, and
SHALL NOT seed example content.

There is nothing to teach beyond the capture gesture: the input language is ordinary text and the
optional syntax is discovered in use. Fabricated example notes in a system meant to hold the user's
own thinking are the wrong first impression and are work to remove.

#### Scenario: Fully local first run

- **WHEN** first run is performed with no network available
- **THEN** it completes and the system is fully usable.

#### Scenario: No sample content

- **WHEN** first run completes
- **THEN** the store contains no notes and the empty state names the capture gesture.
