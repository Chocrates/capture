## Context

M1–M3 built a single-user, multi-device archive: an append-only log, a derived projection, a
recognizer, and a sync protocol with pairing, retirement and redaction propagation. All of it is
archived and all of it runs on Linux.

The phone is not a new data model. It is a second device of a kind the protocol already assumes —
one that is off most of the time, wakes on a schedule, and holds the whole archive. Everything it
needs from the core exists and is exhaustively tested; what does not exist is a way to run that
core on Android, four capture surfaces, and the two features (geofence cues, phone-as-notifier)
that have been specified since M1 with no device able to observe them.

The constraint that shapes almost every decision below is that **Android is a hostile host for a
background process**. Doze suspends services, background execution limits kill work, multicast
receive silently does nothing without a held lock, and background location is a permission the
platform deliberately makes expensive. The protocol was written assuming intermittent reachability
(§7 failure mode 1), which means the correct response is to accept the platform's behaviour rather
than to fight it.

## Goals / Non-Goals

**Goals:**

- Capture on the phone at a cost comparable to the desktop chord: one gesture, no app launch.
- The phone holds the archive, syncs peer-to-peer with the desktop, and needs no server.
- Geofence cues fire on the phone, whatever device is the designated notifier.
- **Full reading parity with the desktop**: timeline, note page, search, agenda, wiki with
  nesting, calendar, review and health. The phone is the device most often to hand, so a phone
  that can only write is one that sends you to a laptop to read what you just wrote.
- One implementation of every rule. Kotlin renders; it does not decide.

**Non-Goals:**

- **Transcription on the phone.** Voice commits a blob reference and stops. The desktop transcribes
  when the blob arrives (§2.2). This is not a simplification to revisit — it is where the phone's
  job ends.
- **Enrichment on the phone.** Android background limits make it unreliable, and it is M5's scope.
- **Any change to the desktop capture gesture.** §8.10 freezes it for this milestone.
- **Any change to the log, the conflict rules, or the wire protocol.** Pressure to change them is a
  signal that something is wrong with the phone, not with the model.
- **A phone-only archive.** The phone is a replica, not the origin of truth — though nothing in the
  design privileges the desktop either.

## Decisions

### The FFI surface is coarse, and shaped like the CLI rather than like the store

uniffi over `capture-core` and `capture-store`, exposing operations — capture, timeline, note,
search, agenda, pair, sync-now — rather than the store's types.

*Why:* every fine-grained surface invites Kotlin to assemble behaviour out of primitives, and the
first thing assembled that way will be something the fold already decides. The conflict rules were
property-tested for two milestones on the assumption there is one implementation. A coarse surface
makes reimplementation awkward; a fine one makes it natural.

*Alternative considered:* exposing events and folding in Kotlin, which would let the UI be reactive
without crossing the boundary per query. Rejected — it puts the fold in two languages, and §6's
rules are the part of this system least able to survive that.

### The phone is a full replica

It holds the whole log and its own projection, and syncs by the same anti-entropy the desktop uses.

*Why:* §5.5 is explicit that no device is required to be online, and a thin client contradicts
that — it would make the desktop a server in everything but name, and the phone useless exactly
when it is most likely to be the only device present. Storage is not the constraint: the archive is
events, and the M3 cost analysis put a lifetime of them well inside a phone's budget.

*Alternative considered:* a windowed replica holding recent events only (the `leaf` role §5.5
names). Rejected for M4 — the role is unbuilt, untested, and its failure mode is silently missing
data, which is the worst thing this system can do.

### Sync runs under WorkManager, and the app holds a multicast lock only while in the foreground

Periodic anti-entropy through `WorkManager` with network and battery constraints; discovery only
while the app is open, holding a `WifiManager.MulticastLock`.

*Why:* the lock is required for multicast receive and, per §8.5, its absence produces no error at
all — discovery simply never finds anything, which is indistinguishable from no peers being
present. Holding it continuously would cost battery for a device that is asleep most of the time.
Foreground-only discovery plus remembered addresses matches how the desktop already falls back
when mDNS is blocked, so the phone uses a path that is already exercised.

*Trade-off accepted:* a phone that has never seen the desktop on the current network cannot find
it without opening the app. That is a real limitation and is stated rather than hidden.

### Capture writes through the FFI synchronously, off the main thread

The capture surfaces call into Rust on `Dispatchers.IO` and commit before returning.

*Why:* §2.1's promise is that a capture the interface accepted is never lost. Queuing in Kotlin and
committing later would move the durability boundary into the layer with the least testing and the
most lifecycle hazards — a Quick Settings tile can be destroyed the moment its callback returns.

### Geofence cues use the platform's geofencing API, and degrade to nothing without permission

*Why:* background location is a permission Android deliberately makes expensive, and it is
reasonable for someone to refuse it. A cue that cannot fire must be visibly inert rather than
silently absent — §2.4 is explicit that an intent nobody can see failing is worse than no intent.

*Decision:* geofence cues are offered only after the permission is granted, and an existing
`location` cue on a phone without permission is reported as unfirable in the health surface,
alongside the other things that are true and unwelcome.

### minSdk 30, target 34

*Why:* the Quick Settings tile is only reachable from the lock screen with `TileService` behaviour
that stabilised around API 30, and both installed SDK platforms (30, 34) bracket that. Supporting
older devices would mean a second capture path for the one surface that matters most.

### Android builds are their own `make` target, not part of `make ci`

*Why:* `make ci` is what runs before every commit and takes minutes already. A Gradle build needs
the SDK, the NDK and a JVM, none of which the Rust gates need, and a CI that cannot run without an
Android SDK is one that stops being run. `make android` builds and tests it; `make ci` states that
it does not, so the omission is a decision rather than an oversight.

## Risks / Trade-offs

**The FFI boundary becomes a place where rules get reimplemented** → The surface is coarse and
operation-shaped, and a test asserts the Kotlin side contains no date arithmetic, no conflict
resolution and no slug generation. This is the same failure the `unwired` gate was built for: the
second implementation is the one nobody exercises.

**Doze makes sync latency unpredictable** → Accepted by design. The protocol assumes intermittent
reachability, and the surfaces report *when* a peer was last reached rather than implying currency.
The phone showing stale data is correct; the phone claiming freshness it does not have is not.

**Discovery fails silently without the multicast lock** → The lock is acquired explicitly and its
absence is a reported condition, not an inferred one. This is written down because the failure has
no symptom: the design records it as a known trap precisely because it looks like "no peers".

**Two capture platforms inside the habit-formation window** (§0.2) → The desktop gesture is frozen
and the phone's surfaces are additions rather than changes, so nothing already in muscle memory
moves. Whether that is sufficient is not knowable in advance and is stated as such.

**A second language in the repository** → Kotlin cannot be held to the same gates as Rust. The
mitigation is that it holds as little as possible: no rules, no parsing, no time arithmetic. If the
Kotlin grows past rendering and lifecycle, that is the signal the FFI surface is too fine.

**Pairing a phone could write a preference nobody chose** → §8.9 forbids it and an earlier draft of
the design did exactly this. The notifier stays a compiled-in default rule until the user picks,
and there is a test asserting no `setting` event is emitted by pairing.

## Migration Plan

Nothing to migrate. The phone joins an existing archive by the pairing gesture built in M3, and
receives the log by the anti-entropy built in M3. If the phone is wrong, it is retired by the
mechanism built in M3 — which is the first time that mechanism will have been used for its actual
purpose rather than a test.

Rollback is uninstalling the app and retiring its key.

### Reading is at parity with the desktop, not a subset

Timeline, note page, search, agenda, wiki with nesting, calendar, review and health.

*Why:* §8.10's M4 line says "capture surfaces" and is silent on reading, so this is deliberately
more than the plan promised. The phone is the device most often within reach; one that can only
write sends you to a laptop to read what you wrote thirty seconds ago, which is the gradient §8.4
argues against in the desktop context and which applies here with more force.

*Cost, stated:* roughly doubles the Compose work, and the calendar grid and wiki sidebar were the
fiddliest parts of the desktop surface. Accepted, because the alternative is a device that is
trusted for capture and not for anything else.

### Acceptance is split by what the emulator can honestly prove

Capture, pairing, sync, convergence and retirement are proven on the emulator, which is
deterministic and scriptable. Doze behaviour, geofence firing and lock-screen tile reachability
require a physical device.

*Why:* those three are precisely what the emulator models least faithfully, and they are also the
three most likely to be broken in the field. A simulated geofence proving a cue fires is much
weaker evidence than a real one, and recording it as proof would be the same category of error as
a passing test over a mechanism nothing calls.

## Open Questions

- **Hold-to-talk versus the assistant intent** — both are specified, and the assistant path depends
  on an integration surface that has changed repeatedly across Android versions. It may be right to
  build hold-to-talk first and treat the assistant intent as optional rather than blocking on it.
- **Whether the phone should hold blobs it did not record.** Audio captured on the phone must live
  there until it syncs; audio captured elsewhere need not come back. §5.6's blob plane is built but
  the phone is the first device where the distinction has a cost.
