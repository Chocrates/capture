## 1. The toolchain, before any of it is interesting

- [ ] 1.1 Add `cargo-ndk` and pin the NDK version. The two installed NDKs differ and a build that silently picks one is a build that differs between machines.
- [ ] 1.2 Cross-compile `capture-store` to `aarch64-linux-android` and run its existing test suite on an emulator. **Before writing any binding.** SQLite, the filesystem and the clock all behave differently there, and finding that out through a Kotlin stack trace is the expensive way.
- [ ] 1.3 `make android` — build and test, kept out of `make ci` deliberately. `make ci` states the omission so it reads as a decision rather than an oversight.
- [ ] 1.4 Record which SDK, NDK and JDK the build needs, and fail with that list rather than a linker error when one is missing.

## 2. The binding

- [ ] 2.1 Create `crates/capture-ffi`. Operation-shaped, not type-shaped: capture, note, timeline, search, agenda, wiki, calendar, review, health, pair, retire, sync-now.
- [ ] 2.2 Generate Kotlin with uniffi and call every operation from a JVM test, before any UI exists. A binding exercised only through a screen is one whose failures arrive as blank screens.
- [ ] 2.3 **Test that a capture is durable before the call returns** — kill the process immediately after and assert the event is in the log. A Quick Settings tile can be destroyed the moment its callback returns, which is exactly when a queue in Kotlin would lose it.
- [ ] 2.4 **The mechanical check that Kotlin decides nothing**: no date arithmetic, no conflict resolution, no slug generation, no recurrence expansion. This is the `unwired` lesson in another language — a second implementation is the one nobody exercises, and both sides look right in isolation.

## 3. Storage and identity on the phone

- [ ] 3.1 Store in app-private storage. Confirm what survives an app update and what does not, and write down which.
- [ ] 3.2 Mint an identity on first run, exactly as `capture init` does. A phone is a device, not a session.
- [ ] 3.3 **Test a restore**: reinstall over an existing store and assert the origin is unchanged, or that a new incarnation is claimed properly if it is not. Restore is the path most likely to be taken in an emergency and least likely to have been run.
- [ ] 3.4 Spool captures when the store cannot be written, and surface it. Same promise as the desktop, more load-bearing here: the platform kills applications routinely and without warning.

## 4. Capture surfaces

- [ ] 4.1 Quick Settings tile, reachable from the lock screen. The one surface that has to be faster than opening anything.
- [ ] 4.2 Share-sheet target, attaching the source URL where the sharing application offers one.
- [ ] 4.3 Hold-to-talk: record, commit a blob reference, stop. **No transcription on the phone** — the phone's job ends at "the audio exists".
- [ ] 4.4 Assistant voice intent. Last of the four deliberately: it depends on an integration surface that has changed repeatedly across Android versions, and the other three do not.
- [ ] 4.5 **Measure what each surface costs**, gesture to committed. The desktop's chord has a latency histogram; the phone should be able to answer the same question rather than assert it.

## 5. Reading

- [ ] 5.1 Timeline, note page, search.
- [ ] 5.2 Agenda and calendar.
- [ ] 5.3 Wiki with nesting.
- [ ] 5.4 Review session.
- [ ] 5.5 Health, including peers and what is outstanding against each.
- [ ] 5.6 Every mutation the desktop offers, per §8.4. A phone that suppresses a note only by reaching for a laptop inverts the gradient §2.6 exists to create.

## 6. Sync

- [ ] 6.1 Anti-entropy under `WorkManager`, with network and battery constraints.
- [ ] 6.2 **Hold a `WifiManager.MulticastLock` while browsing, and report when it cannot be acquired.** Without it multicast receive produces no error and no packets — discovery appears to run and finds nothing, which is indistinguishable from an empty network. This is the trap the design names by name.
- [ ] 6.3 Remembered addresses as the fallback, the same path the desktop uses where mDNS is blocked.
- [ ] 6.4 Pair the phone to the desktop with the six-digit gesture. **Test that pairing emits no `setting`** — §8.9, and an earlier draft of the design got this exactly wrong.
- [ ] 6.5 **Test convergence with the desktop**: capture on each while apart, reconcile, assert identical folds. This is M3's acceptance test with a real second platform rather than a second directory.
- [ ] 6.6 Report staleness as *when*, never as freshness. The device that sleeps most is the one that must not imply currency.

## 7. Cues and notification

- [ ] 7.1 Geofence cues through the platform API, offered only once location permission is granted.
- [ ] 7.2 **A `location` cue fires on the phone whatever device is the notifier.** Routing it to a designated notifier delivers "you are at the hardware shop" to a desktop at home.
- [ ] 7.3 **An existing `location` cue on a phone without permission is reported as unfirable**, and keeps being reported. An intent failing invisibly is worse than no intent: the user believes something is watching when nothing is.
- [ ] 7.4 The phone as notifier, chosen in the UI, authored as a `setting` — and never authored by pairing.
- [ ] 7.5 Devices announce whether they can notify, so the choice is made among devices that can deliver.
- [ ] 7.6 **Test that declining location leaves capture, sync and reading untouched.**

## 8. Acceptance

- [ ] 8.1 On the emulator: capture from each surface, pair, sync, converge, retire. Scripted and repeatable.
- [ ] 8.2 On a physical device: the tile from the lock screen, a geofence actually firing, and behaviour across a real doze cycle. These are what the emulator models least faithfully and what is most likely to be broken in the field — a simulated geofence is much weaker evidence than a real one.
- [ ] 8.3 Confirm the desktop capture gesture is unchanged — chord, overlay, commit key, latency histogram. §8.10 freezes it for this milestone.
- [ ] 8.4 `make ci` still passes, and `make android` passes alongside it.
- [ ] 8.5 Use it for a week as the primary capture surface, and record what makes capture feel slow or uncertain. This is M1's item 13.6 finally answerable, on the device where most thoughts actually arrive.
