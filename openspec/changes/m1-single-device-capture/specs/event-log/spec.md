## ADDED Requirements

### Requirement: The event envelope is fully specified and frozen

The system SHALL define every envelope field before the first event is written: `id`, `dev`, `seq`,
`hlc`, `kind`, `tgt`, `prev`, `occ`, `chain`, `bhash`, `body`, `sig`. Every field except `body` and
`sig` is covered by `id` and is therefore permanently frozen.

The system SHALL also byte-specify the body of every event kind it writes. Bodies are hashed into
`bhash`, which is hashed into `id`, which is signed — so a body shape is frozen on first use, and
`envelope_version` does not cover it. A breaking change to a body requires a new kind name.

#### Scenario: Envelope round-trips

- **WHEN** an event is encoded and decoded
- **THEN** every field is recovered exactly and the recomputed `id` matches.

#### Scenario: Body shapes are specified before use

- **WHEN** an event kind is written for the first time
- **THEN** its body schema exists in the specification and has committed test vectors.

### Requirement: `tgt` identifies the note; `prev` identifies the version

The system SHALL set `tgt` on every event that acts on a note, to the id of that note's root
capture, and SHALL index it so that folding one note is a single indexed lookup. On a `capture`,
`tgt` is the event's own id. `prev` SHALL be set on mutations only, naming the exact version being
superseded, and is null on a capture.

These are different fields for different jobs. Walking `prev` to discover which note an event
belongs to would require a chain walk on every fold and breaks entirely on a replica holding an
edit but not the root.

#### Scenario: Folding one note is a direct lookup

- **WHEN** a note's state is computed
- **THEN** its events are found by `tgt` without walking any predecessor chain.

#### Scenario: Capture is its own target

- **WHEN** a capture event is written
- **THEN** `tgt` equals its own `id` and `prev` is null.

### Requirement: Each origin's events form a hash chain

The system SHALL set `chain` on every event to the `id` of the previous event authored by the same
origin, and SHALL verify the link on ingest where the predecessor is held. An event whose
predecessor is not yet held SHALL be quarantined rather than rejected.

The chain is what makes an origin's history verifiable independently of any timestamp the origin
asserts about itself.

#### Scenario: Chain links to the predecessor

- **WHEN** an event is appended at sequence n
- **THEN** its `chain` equals the id of that origin's event at sequence n−1.

#### Scenario: Truncation is detectable

- **WHEN** an event is removed from an origin's stream
- **THEN** a chain walk over that origin detects the break.

#### Scenario: Substitution is detectable

- **WHEN** an event in an origin's stream is replaced with a different one
- **THEN** a chain walk detects that its successor's `chain` no longer matches.

#### Scenario: Missing predecessor is quarantined

- **WHEN** an event arrives before the event it chains to
- **THEN** it is quarantined and re-evaluated when the predecessor arrives.

### Requirement: Only the originating device authors under its origin

The system SHALL author events only under its own `dev`, and SHALL never allocate a sequence number
in another origin's space.

This is the property that makes merging a set union: two origins can never contend for a position,
so there is no write conflict to resolve at the log level.

#### Scenario: Sequence allocation is local only

- **WHEN** events from another origin are ingested
- **THEN** the local sequence counter is unaffected.

### Requirement: The hybrid logical clock is specified and frozen

The system SHALL stamp every event with an HLC composed of physical milliseconds, a logical
counter, and the origin id as final tiebreak, and SHALL byte-specify its layout and comparison
before the first event is written. The comparison SHALL yield a total order over all events.

The HLC is a signed, frozen header field, it is the order the fold sorts by, and it is the tiebreak
for every last-writer-wins rule. It is as irrevocable as the identity construction.

#### Scenario: Total order

- **WHEN** any two distinct events are compared
- **THEN** exactly one orders before the other, and the result is identical on every device.

#### Scenario: Same millisecond

- **WHEN** two events are authored within the same physical millisecond by one origin
- **THEN** the logical counter distinguishes them and preserves authoring order.

### Requirement: The clock is bounded at emission, adopted on receipt

The system SHALL refuse to emit an event whose HLC would exceed its own wall clock by more than a
defined tolerance, and SHALL surface a peer beyond tolerance as a condition requiring attention. On
receipt the system SHALL advance its logical clock to at least the incoming value — adopting it,
never clamping it.

Adoption is what preserves causality: an event that has seen another must order after it. Clamping
an incoming value breaks that, so a later edit can sort before the version it edits — which
silently loses every last-writer-wins race for the duration of the skew. Filtering future-dated
events out of the fold instead makes the projection depend on the local clock, so the same log
renders differently on different machines. Both were tried and both are wrong.

#### Scenario: Skewed emission is refused

- **WHEN** the local clock is materially ahead of real time and an event is authored
- **THEN** the event is not emitted with that timestamp and the condition is surfaced.

#### Scenario: Causality survives ingest

- **WHEN** an event is ingested whose timestamp is ahead of the local clock, and a reply is then
  authored locally
- **THEN** the reply orders after the event it replies to.

#### Scenario: No event is excluded from the fold for its timestamp

- **WHEN** an event carries any timestamp whatsoever
- **THEN** it is folded on every replica; the projection never varies by local clock.

### Requirement: The clock survives restart

The system SHALL persist the last emitted HLC and SHALL resume from at least that value after a
restart, including when the wall clock has moved backwards.

Without this, a clock correction or a reboot produces events that duplicate or precede timestamps
already written — on a single device, in the first week.

#### Scenario: Clock steps backwards

- **WHEN** the system restarts after the wall clock has moved backwards
- **THEN** newly emitted events still order after every previously emitted event.

### Requirement: Canonical encoding is frozen

The system SHALL encode event bodies and headers using deterministic CBOR with a fixed field
order, and SHALL treat that encoding as permanently frozen. Because event ids are hashes over the
encoding, any change to it would change every id in the log and fork the archive. An
`envelope_version` field SHALL exist for the case where the envelope shape itself must change;
altering it is a deliberate, converted fork.

#### Scenario: Identical events encode identically across platforms

- **WHEN** the same logical event is encoded on two different platforms
- **THEN** the byte sequences are identical and both produce the same `id`.

#### Scenario: Field order does not depend on map iteration

- **WHEN** an event body is encoded twice within one process
- **THEN** the byte sequences are identical regardless of internal map ordering.

### Requirement: Event identity covers the whole envelope

The system SHALL compute `id = blake3(canonical header ‖ bhash)`, where the header is every
envelope field except `body` and `sig`, and `bhash = blake3(canonical body)`. The signature SHALL
be Ed25519 over `id`.

Hashing the body alone would leave `dev`, `seq`, `hlc`, `kind`, `tgt` and `prev` unsigned, allowing
any party that has seen the log to re-emit a signed body under a different origin, kind or target;
it would also make two identical captures collide on the primary key.

#### Scenario: Routing fields are authenticated

- **WHEN** an event is received whose `kind` or `tgt` differs from what its author signed
- **THEN** the recomputed `id` does not match and the event fails verification.

#### Scenario: Identical bodies do not collide

- **WHEN** the same text is captured twice on the same device
- **THEN** the two events have different `seq` and `hlc`, therefore different ids, and both are stored.

### Requirement: Every body carries a random nonce

The system SHALL prefix every canonical body with a 16-byte cryptographically random nonce.

Without it, `bhash` is an unsalted commitment to content that redaction destroys, and it survives
in every replica, export and backup. The content redaction exists for — a pasted password, a third
party's details — is short and low-entropy, so an adversary holding any export could confirm a
guess by hashing candidates.

#### Scenario: Two captures of identical text produce different body hashes

- **WHEN** the same text is captured twice
- **THEN** the two events have different `bhash` values.

#### Scenario: Nonce is destroyed with the body

- **WHEN** an event is redacted
- **THEN** the nonce is destroyed along with the rest of the body, and `bhash` no longer permits confirming a guess about the content.

### Requirement: Body integrity is verified on ingest

The system SHALL verify `blake3(body) == bhash` on every ingested event that carries a body, and
SHALL reject events that fail. Verification SHALL be skipped only where `body IS NULL` because the
event has been redacted.

Because `id` and `sig` authenticate the body's *hash* rather than the body, omitting this check
would allow a relaying device to substitute different content that still verifies.

#### Scenario: Substituted body is rejected

- **WHEN** an event arrives whose body does not hash to its `bhash`
- **THEN** it is rejected, recorded in the rejected table, and never stored in the event log.

#### Scenario: Redacted rows still verify

- **WHEN** `capture verify` runs over a log containing redacted events
- **THEN** those rows' ids recompute and signatures verify, and the body check is skipped for them.

### Requirement: Origin identity includes a random incarnation

The system SHALL derive origin identity as `dev = H(pubkey ‖ incarnation)` where the incarnation is
a random 128-bit value minted on first run and re-minted on every restore, reinstall, or database
replacement.

An incrementing counter would have to be read from the artifact that was just lost, so two restores
from the same or a stale export would mint the same origin and collide at every sequence position.

#### Scenario: Restore produces a distinct origin

- **WHEN** a store is restored from an export and a fresh incarnation is minted
- **THEN** the restored device authors under a new `dev` starting at `seq` 1 and collides with nothing.

#### Scenario: Two restores from one export do not collide

- **WHEN** the same export is restored onto two machines
- **THEN** each mints an independent random incarnation and the two origins are distinct.

### Requirement: Append is atomic and gap-free

The system SHALL allocate `seq`, insert the event row, and update `heads` in a single transaction,
and SHALL commit durably before acknowledging a capture. `seq` SHALL be gap-free per origin.

#### Scenario: Crash between allocation and insert

- **WHEN** the process crashes mid-append
- **THEN** on restart either the whole event is present or none of it is, and no sequence gap exists.

#### Scenario: Acknowledged captures are durable

- **WHEN** a capture is acknowledged to the user and the machine immediately loses power
- **THEN** the event is present after restart.

### Requirement: Heads track the highest contiguous sequence

The system SHALL maintain `heads[dev]` as the highest *contiguous* sequence held for that origin,
never as a maximum, and SHALL record out-of-order events as pending until the gap fills.

A maximum silently loses data: an interrupted range transfer that has already delivered a later
event would leave `heads` advanced past events that are then never requested again.

#### Scenario: Out-of-order arrival does not advance heads

- **WHEN** event `seq` 41823 is ingested while 41601–41822 are missing
- **THEN** `heads` remains at 41600 and 41823 is recorded as pending.

#### Scenario: Filling a gap absorbs pending events

- **WHEN** the missing range is subsequently ingested
- **THEN** `heads` advances past the previously pending events in one step.

### Requirement: Re-ingesting an event is a no-op only when the ids match

The system SHALL treat an incoming event whose `(dev, seq)` is already held as a duplicate **only
if its `id` is also identical**. If the id differs, two different events claim one position in a
single origin's stream, which single-writer-per-origin says is impossible; the system SHALL
quarantine it loudly rather than discarding it.

Treating that case as a benign duplicate is what turns a rolled-back store into silent permanent
data loss in both directions. This is reachable in M1: restore replays events through the ordinary
ingest path, and replaying overlapping exports is routine.

#### Scenario: True duplicate is idempotent

- **WHEN** the identical event is ingested twice
- **THEN** the second ingest changes nothing and reports success.

#### Scenario: Position collision is loud

- **WHEN** an event arrives claiming a `(dev, seq)` already held by an event with a different `id`
- **THEN** it is quarantined, surfaced as requiring attention, and neither event is discarded.

### Requirement: Unauthenticated events never occupy a position

The system SHALL store events that fail signature verification, `bhash` verification, or origin-key
lookup in a separate rejected table keyed by `id` alone. Such events SHALL NOT occupy a
`(dev, seq)` position, SHALL NOT advance `heads`, and SHALL NOT be relayed.

Allowing an unauthenticated event to claim a position lets any peer squat a sequence number, after
which the genuine event at that position can never be accepted.

#### Scenario: Forged event does not squat a position

- **WHEN** an event with an invalid signature claiming `seq` 15678 is ingested
- **THEN** it is recorded as rejected, `heads` does not advance, and the genuine `seq` 15678 is accepted normally when it arrives.

#### Scenario: Rejected events are visible

- **WHEN** any event has been rejected
- **THEN** it is reported in the health surface as a tier-1 item, not silently discarded.

### Requirement: Authentic but unfoldable events are quarantined

The system SHALL quarantine events whose signature verifies but which cannot be folded — unknown
kind, malformed body, origin not yet admitted, or a missing chain predecessor. Quarantined events
SHALL be stored at their position, MAY advance `heads`, SHALL be relayed, and SHALL be excluded
from the fold.

Rejecting them instead would leave a permanent sequence gap that anti-entropy re-requests forever.

#### Scenario: Unknown event kind is retained

- **WHEN** an event of an unrecognised kind is ingested and its signature verifies
- **THEN** it is stored and quarantined, and `heads` may advance past it.

#### Scenario: Quarantine is re-evaluated

- **WHEN** a client upgrade adds support for a previously unknown kind, or a `device_add` admits a previously unknown origin
- **THEN** the affected quarantined events are re-evaluated and folded without user action.

### Requirement: Schema evolution never migrates the log

The system SHALL evolve by adding fields and new event kinds only. Existing kinds SHALL NOT change
meaning; a breaking change SHALL take a new kind name. The system SHALL NOT refuse to interoperate
with a peer on version grounds.

#### Scenario: Unknown field is ignored

- **WHEN** an event carries a field this client does not recognise
- **THEN** the field is ignored, the event is folded normally, and the field is preserved in storage.

#### Scenario: No version floor

- **WHEN** a peer runs a client several versions old
- **THEN** it participates fully in the log; it quarantines what it cannot interpret rather than refusing.

### Requirement: Integrity is independently verifiable

The system SHALL provide a verification pass that recomputes every event id, checks every
signature, checks `blake3(body) == bhash` where a body exists, and walks each origin's hash chain.

#### Scenario: Clean log verifies

- **WHEN** verification runs over an intact log
- **THEN** it reports success and exits zero.

#### Scenario: Tampered row is detected

- **WHEN** a stored event's body is altered outside the application
- **THEN** verification reports that row as failing and exits non-zero.

#### Scenario: Rejected events are not integrity failures

- **WHEN** verification runs on a store that has recorded rejected events
- **THEN** they are reported in the summary but do not by themselves cause a non-zero exit.
