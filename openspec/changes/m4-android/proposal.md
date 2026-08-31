## Why

Every capture surface so far is on a machine you have to be sitting at. The design's premise is
that capture must cost almost nothing at the moment a thought arrives (§2.1), and most thoughts do
not arrive at a desk. Until the phone captures, the system is only testable in the half of life
that happens in a chair — which is why the archive on the primary device is nine notes after three
milestones.

M4 is also where two features that have been specified since M1 become observable for the first
time. Geofence cues cannot fire anywhere but the phone, and the phone is what the notifier
designation exists to point at (§2.4.1). Both have been carried as design for three milestones
with nothing able to exercise them.

## What Changes

- **A `capture-ffi` crate**: a uniffi surface over `capture-core` and `capture-store`, generating
  Kotlin. Hand-written JNI for a surface this size would be a permanent tax (§8.4).
- **An `android/` app**: Kotlin + Jetpack Compose, linking `libcapture.so`, over the same store
  and the same log as the desktop. Not a client of the desktop — a replica.
- **Four capture surfaces on the phone**, per §2.2: a Quick Settings tile reachable from the lock
  screen, a share-sheet target that attaches the source URL, an assistant voice intent, and
  hold-to-talk. Voice records locally and commits a blob reference; it does **not** transcribe on
  device — the phone's job ends at "the audio exists".
- **Reading surfaces**: timeline, note page, search, agenda. Enough to answer "what did I write"
  without a laptop.
- **Sync on the phone**: the existing protocol, unchanged, with the two Android-specific
  requirements the design already names — a `WifiManager.MulticastLock` held during discovery, and
  `WorkManager` constraints for background rounds.
- **Geofence cues** (`location`), firing on the phone regardless of which device is the notifier.
- **The phone as notifier**, selectable in the UI. Pairing an Android device **must not** emit a
  `setting` on the user's behalf.
- **No enrichment on the phone.** Android background execution limits make it impossible, and it
  is M5's scope anyway. The phone captures; the desktop enriches.
- The desktop capture gesture — chord, overlay, commit key — is **frozen** for this milestone
  (§8.10) and must not change.

## Capabilities

### New Capabilities

- `android-app`: what the phone application is, what it shows, and what it does not do — a replica
  holding the whole archive rather than a thin client, with no enrichment and no transcription.
- `android-capture`: the four capture surfaces on the phone and what each attaches, including the
  rule that voice commits a blob reference rather than a transcript.
- `android-ffi`: the boundary between Rust and Kotlin — what crosses it, what stays on the Rust
  side, and the rule that no conflict rule or fold logic is reimplemented in Kotlin.
- `android-background`: discovery and sync under doze and background execution limits, including
  the multicast lock without which discovery fails silently.
- `location-cues`: geofence cues — where they fire, why they cannot be relocated, and what happens
  when the phone is not the designated notifier.

### Modified Capabilities

- `intents`: the notifier designation becomes selectable rather than implied, and local cues are
  stated as overriding it. The existing spec assumes one device.
- `sync-discovery`: Android requires a held multicast lock for mDNS receive, which is a
  platform-specific precondition the current spec does not carry.

## Impact

**New**: `crates/capture-ffi/`, `android/`. New build tooling — `cargo-ndk`, Gradle, the Android
SDK/NDK — and a second language in the repository, which `make ci` must cover or explicitly
exclude with a reason.

**Unchanged, deliberately**: `capture-core`, `capture-store`, `capture-sync` and the event log.
M4 adds a device, not a data model. The conflict rules, the wire protocol and the frozen encodings
were settled in M1–M3 and this milestone is a consumer of them. Any pressure to change them is a
signal that something is wrong with the phone, not with the model.

**Risk carried from the design**: a second `capture` surface on a second platform lands inside the
66–154 day window §0.2 says context stability matters most in. The mitigation is the same one M1
chose — the desktop gesture is frozen — and the phone's surfaces are new rather than changed, so
nothing already in muscle memory moves.
