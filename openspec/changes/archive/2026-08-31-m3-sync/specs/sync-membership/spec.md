## ADDED Requirements

### Requirement: A device is paired once, with any one existing device

The system SHALL admit a new device by pairing it out of band with a single
existing device, which appends a `device_add` event. Every other device SHALL
learn of the new member by folding the log. The system SHALL NOT require
pairing each device with each other device.

**The gesture is six digits, shown on one device and typed into the other.**
§5.3 specified a mutual QR exchange, which assumes a camera at both ends and
the desktop has none. The code is turned into a strong key by SPAKE2 and
confirmed by a `Noise_NNpsk0` handshake: used directly as a key, six digits
would be a million guesses tried offline in seconds by anyone who recorded the
exchange, and the handshake would confirm the right guess for them. With a
PAKE a wrong code yields a different key and no information about the right
one, so guessing must be online, one attempt at a time.

**Both sides admit the other.** Each device that has been initialised has a
roster root of its own, and only one root confers membership once two archives
meet. If pairing admitted in one direction only, the device whose root lost the
ordering would not be a member of the combined roster, and every event it
authored would be unverifiable on the other — permanently, silently, and about
half the time, since which root wins depends on generated key bytes.

#### Scenario: Pairing admits in both directions
- **WHEN** two initialised devices are paired
- **THEN** each SHALL appear in the other's roster
- **AND** this SHALL hold whichever root wins the ordering

#### Scenario: A third device
- **WHEN** a third device is paired with the laptop only
- **AND** the desktop later syncs with the laptop
- **THEN** the desktop SHALL admit the third device without a further gesture

### Requirement: Admission is visible

The system SHALL surface a `device_add` for a key this device neither paired nor
emitted as a condition needing attention (§4.5 tier 1), acknowledgeable but not
silently dismissed. This is possible because `device_add` is an ordinary event:
a device admitted without the user's knowledge is a fact in the log rather than
a state nobody can see.

#### Scenario: An unexpected admission
- **WHEN** a `device_add` arrives for a key this device did not pair and did not
  itself emit
- **THEN** it SHALL be reported as needing attention
- **AND** SHALL remain reported until acknowledged

#### Scenario: An expected admission
- **WHEN** this device performed the pairing itself
- **THEN** the resulting `device_add` SHALL NOT be reported as unexpected

### Requirement: Pairing resists an attacker on the same network

The pairing gesture SHALL require something an attacker on the LAN does not
have. Proximity on the network SHALL NOT be sufficient to join a roster.

#### Scenario: A device on the network that was not invited
- **WHEN** an uninvited device attempts to pair
- **THEN** pairing SHALL fail without the out-of-band secret

### Requirement: A lost device can be retired, and a lockout undone

The system SHALL retire a device by **key** rather than by origin, naming a
head per origin taken from the log rather than supplied by hand. Retiring an
origin would let a stolen device mint a fresh incarnation on its next restart
and return; a head supplied by hand would be a guess about another device's
history.

The head SHALL be enforced by the per-origin chain rather than by timestamp. A
retired device chooses its own `hlc` and can backdate, and there is no causal
ordering between a retirement and the events of the device being retired.

Events beyond the head SHALL be set aside rather than rejected: a version
vector cannot represent a hole, so a refusal would leave a gap that
anti-entropy re-requests from every peer for ever.

#### Scenario: A backdated event from a retired device
- **WHEN** a retired device authors an event past its named head and stamps it
  before the retirement
- **THEN** it SHALL be set aside
- **AND** the origin's numbering SHALL remain gap-free

#### Scenario: A device does not accept being retired by another
- **WHEN** a retirement of this device arrives that this device did not issue
- **THEN** this device SHALL continue to treat itself as a member
- **AND** its peers SHALL continue to refuse it, which is what retiring it
  achieved

#### Scenario: Two signatures undo a lockout
- **WHEN** every device has been retired
- **THEN** a `device_readmit` signed by two different keys SHALL restore
  membership
- **AND** SHALL be valid even though both signers are retired
