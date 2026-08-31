## 1. The toolchain, before any of it is interesting

- [x] 1.1 Add `cargo-ndk` and pin the NDK version. **Done**, and the reason turned out to be sharper than expected: of the two NDKs present one is `28.0.12433566-beta1`. Resolving "newest" would have silently selected a beta toolchain.
- [x] 1.2 Cross-compile `capture-store` and run its existing test suite on an emulator. **Before writing any binding.** **Done, and it passes unmodified**: 223 unit tests and 108 integration tests, including the whole `two_devices` suite — pairing, sync, convergence, retirement, redaction — over real sockets on Android. SQLite's WAL, the writer lock, the spool and the watch all behave as they do on Linux. The emulator is x86_64, so that target had to be added; the installed `aarch64`/`armv7` targets only build for a physical phone.
  - Two suites do not run on a device and are listed rather than skipped: `reachable` scans the source tree, which is not there, and `crash` spawns a sibling binary through a cargo path that only exists where cargo built it. Neither is an Android problem and both would otherwise look like one.
- [x] 1.3 `make android` — build and test, kept out of `make ci` deliberately, and `ci` now prints why.
- [x] 1.4 Record which SDK, NDK and JDK the build needs, and fail with that list rather than a linker error. **Done** in `tools/android-env.sh`, which had the exact bug it exists to prevent: written `#!/bin/sh` but sourced into whatever shell the caller runs, and zsh does not word-split unquoted expansions — so it read one target named `"x86_64-linux-android aarch64-linux-android"` and declared it missing. Now verified identical under sh, zsh and bash.

## 2. The binding

- [~] 2.1 Create `crates/capture-ffi`. Operation-shaped, not type-shaped. **Done for capture, reads and every mutation** — capture, note, timeline, search, note_count, device_key, edit, tag, untag, set_alias, pin, mute, set_parent, redact, cancel_redaction, set_done, settle. Agenda, wiki, calendar, review, health, pair, retire and sync-now still to add; they are more of the same shape rather than new questions.
- [x] 2.2 Generate Kotlin with uniffi and call every operation from a test, before any UI exists. **Done**: 6 instrumented tests pass on the emulator through the generated Kotlin, loading a real `.so` through JNA on a real Android runtime. The Rust suite says nothing about any of that layer — and it was the layer that broke first: `Archive(root)` compiles in the head and fails, because uniffi puts a `NoHandle` constructor on the class for fakes and the real one on the companion as `Archive.open()`. Only reading the generated Kotlin said so.
- [x] 2.3 **Test that a capture is durable before the call returns** — kill the process immediately after and assert the event is in the log. A Quick Settings tile can be destroyed the moment its callback returns, which is exactly when a queue in Kotlin would lose it. **Done**: the handle is dropped immediately after capturing and the archive reopened. A process-local write mutex was needed above the store's own lock — that one protects *between* processes, and eight coroutines racing for it reported "somebody else is writing" to the only process on the device.
- [x] 2.4 **The mechanical check that Kotlin decides nothing**: no date arithmetic, no conflict resolution, no slug generation, no recurrence expansion. This is the `unwired` lesson in another language — a second implementation is the one nobody exercises, and both sides look right in isolation. **Done**: `make android-decides`, which names the file, the line and the reason. A rule whose reason is not on screen when it fires gets suppressed rather than obeyed. Verified against a planted violation.

## 3. Storage and identity on the phone

- [~] 3.1 Store in app-private storage. The tests use `context.filesDir`, which is the right place; what survives an update is still to confirm. `allowBackup` is **off**: the archive carries a per-device identity, and a platform backup restored onto a second device would give two devices one origin and therefore two devices issuing the same sequence numbers — the corruption §1 mints a new incarnation on restore to prevent.
- [x] 3.2 Mint an identity on first run, exactly as `capture init` does. **Done and tested on device**: opening the same directory twice returns the same device key. A fresh identity per open would give one machine many origins and every peer would have to learn each of them.
- [ ] 3.3 **Test a restore**: reinstall over an existing store and assert the origin is unchanged, or that a new incarnation is claimed properly if it is not. Restore is the path most likely to be taken in an emergency and least likely to have been run.
- [ ] 3.4 Spool captures when the store cannot be written, and surface it. Same promise as the desktop, more load-bearing here: the platform kills applications routinely and without warning.

## 4. Capture surfaces

- [x] 4.1 Quick Settings tile, reachable from the lock screen. **Done**: the tile opens the capture window rather than capturing silently — a gesture with nothing in it is not a capture, and a tile that captured without showing anything could not be corrected. Verified on the emulator: typed text, pressed Enter, and a full store appeared on the device with the note in the projection under a proper slug.
  - **Enter did nothing at first.** The field was multi-line, so Enter inserted a newline and `ImeAction.Done` never fired. It looked right, accepted the text, and silently did nothing on the commit key. §0.2 argues the capture gesture must not differ between devices, and "Enter commits on the desktop, inserts whitespace on the phone" is exactly that difference — and the kind nobody reports, because it reads as their own mistake.
- [x] 4.2 Share-sheet target, attaching the source URL where the sharing application offers one. **Done**, and it fills a field §2.3 has carried since M1 with nothing able to write it — the desktop has no share gesture. The URL goes in the envelope, not the text: pasted into the text it would become part of what you wrote, and be searched, edited and redacted as though you had typed it. Also registered for `PROCESS_TEXT`, so selected text in any application can be captured without leaving it.
  - **The first version crashed and lost the shared text.** `Theme.NoDisplay` requires `finish()` before `onResume()` completes, and the capture is asynchronous because it must be committed before reporting success. The platform threw, the process died, and nothing was stored — a failure invisible from the sharing application's side, which shows a share sheet that appears to have worked.
- [~] 4.3 Hold-to-talk: record, commit a blob reference, stop. **No transcription on the phone** — the phone's job ends at "the audio exists". **Blocked on the blob plane, which does not exist**: §5.6 specifies it and M3 did not build it. There is no event kind for a blob, no content-addressed storage, and no `WANT`/`BLOB` messages — the `blobs/` directory is created and nothing writes to it. Voice therefore needs a new frozen event kind, local storage and two wire messages, which is the first change to the log's encodings since M3 was archived. Deferred to section 9 rather than squeezed in here: a frozen encoding decided in a hurry is permanent from its first write.
- [~] 4.4 Assistant voice intent. Deferred with 4.3 — it is a voice surface and needs the same blob plane underneath.
- [ ] 4.5 **Measure what each surface costs**, gesture to committed. The desktop's chord has a latency histogram; the phone should be able to answer the same question rather than assert it.

## 5. Reading

- [x] 5.1 Timeline, note page, search. **Done and driven on the emulator**: notes grouped by the day the archive supplied, with times in each note's own zone.
- [~] 5.2 Agenda and calendar. **Agenda done** — three bands, and the first is not a subdivision of the second (§4.4, §3.6). The calendar grid is still to do, and needs the desktop's column packer moved into the store rather than written twice.
- [x] 5.3 Wiki with nesting. The parent arrives as a field on each page rather than as a tree, so there is nothing on this side that can get a cycle or an orphan wrong.
- [ ] 5.4 Review session.
- [~] 5.5 Health. **Conditions done**, in two tiers that are not severities: `attention` wants a decision, `worth-knowing` needs nothing. Peers are still to add, with sync in section 6.
- [x] 5.6 Every mutation the desktop offers, per §8.4. **Done and driven on the emulator**: edit, tag, rename, move, pin, mute, destroy and cancel, each re-reading the note rather than patching the copy on screen.
  - **A queued destruction could only be cancelled by the process that ordered it.** `cancel_redaction` is keyed by the id `redact` returns, which a desktop holds in its window and a phone does not: the application is destroyed and rebuilt on rotation, on a low-memory kill, on a minute in the background. Every one of those would have committed a destruction that was cancellable a second earlier, without anybody deciding to — §1.1's window closing when the platform felt like it. `pending_redaction` asks the projection for the id instead, and the cancel is now verified across a `force-stop`.
  - The confirmation asks for the note's name typed out. A count is dismissible by the reflex that dismisses dialogs; a name is not. **uiautomator reports the disabled button as `enabled="true"`** — Compose carries disabled state in semantics and leaves the legacy bit set — so the only evidence that the guard holds is tapping it and watching nothing happen.

## 6. Sync

- [~] 6.1 Anti-entropy under `WorkManager`, with network and battery constraints. **Built**: periodic at the platform's fifteen-minute floor, unmetered and not-battery-low, `KEEP` so an application opened often does not sync *less* than one opened rarely. Not yet observed actually firing on a device.
  - The round itself moved out of `capture-syncd` and into `capture-store::round`. A second copy for the phone would be the one nobody exercises, and the divergence would appear as devices that sync perfectly with their own kind. The daemon keeps the timer, the resume signal and the accept loop; it decides *when*, and `round` decides what one is.
  - `round::bounded` takes the window as a parameter, because the caller is about to be killed. It listens as well as dials: a device that only dialled would be unreachable to a peer that woke first.
- [~] 6.2 **Hold a `WifiManager.MulticastLock` while browsing, and report when it cannot be acquired.** **Built**: acquired around the round and released in `finally`, and the report carries whether a responder opened at all — said out loud, because a round that found nothing for want of multicast is a different situation from one where nobody was awake. Not yet confirmed against a real second device.
- [~] 6.3 Remembered addresses as the fallback, the same path the desktop uses where mDNS is blocked. **This is the path `tools/converge.sh` exercises**, and deliberately: the emulator's network is not the host's, they are joined by `adb reverse`, and mDNS does not cross that. So the emulator run proves the static fallback and the physical-device run (8.2) proves discovery — which is the right way round, since the fallback is what has to work on every corporate and guest network and would otherwise only ever be exercised by accident.
- [~] 6.4 Pair the phone to the desktop with the six-digit gesture. **The binding and the §8.9 test are done**; the screen to drive it from is not.
  - **Pairing is two calls, not one.** The first draft returned the code from the call that waits — so the code became available only once pairing had already happened, the one moment it is of no use. `Offer` holds the open window and dropping it closes the socket, which is what should happen when somebody navigates away.
  - **The test asserts pairing wrote no `setting` on either side, and then proves its own instrument** by authoring a setting and checking the reader sees it. "The log contains no events of this kind" passes just as cheerfully when the reader is broken.
  - `peers()` lists the union a sync round considers, not just remembered addresses: a device seconds after admitting another has an address for it nowhere, and the narrower reading showed nothing at all — which reads as pairing having failed.
  - Noted for §7.4: there is **no `notifier` setting key yet**. The known keys are `timeline.limit` and `capture.text_scale`.
- [x] 6.5 **Test convergence with the desktop**: capture on each while apart, reconcile, assert identical folds. **Passing.** A real `capture` process, a real emulator, a six-digit pairing over real sockets, asserted in both directions because neither side can see the other's store. The desktop half runs `capture-syncd` rather than a bespoke exchange, so what is tested is what ships.
  - It waits for the pairing code to appear rather than sleeping a guessed amount. A fixed sleep is a test that passes on a fast machine and fails on a slow one, which is indistinguishable from a flaky product.
  - **`tools/android-env.sh` aborted under `set -u`** before it could report anything — the opposite of what it exists for. Every lookup now expands with `:-`, verified under sh, zsh and bash with `set -eu`.
  - **`adb reverse` cannot be how the two are joined**: it makes adbd bind that port *on the device*, so the phone could no longer bind its own sync listener and failed with "Address already in use" against itself. The emulator already maps the host's loopback to 10.0.2.2, which needs nothing. The phone is the only side that can open a connection, so the daemon starts before the phone syncs rather than after — a daemon started afterwards is a desktop that slept through the exchange.
- [x] 6.6 Report staleness as *when*, never as freshness. Health lists each device with when it was last reached, or "never reached from here" — which is the honest reading for a peer that has only ever dialled in, since the port it called from was ephemeral.

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

## 9. The blob plane (§5.6), before voice can land

Deferred out of section 4 deliberately. Voice capture is not a surface on top
of existing machinery: it needs a frozen event kind, content-addressed storage
and two wire messages, and it is the first change to the log's encodings since
M3 was archived. A frozen encoding decided in a hurry is permanent from its
first write.

- [ ] 9.1 Content-addressed local storage under `blobs/`, keyed by `blake3`. Verification is then free and duplicates impossible.
- [ ] 9.2 An event kind referencing a blob, with a golden vector **before anything writes one** — the same discipline every other body got.
- [ ] 9.3 `WANT`/`BLOB` inside the Noise session, 32 KiB chunks so one fits in a transport message. **Not a separate unencrypted byte-serving path** — §5.2 names that as the easy mistake, and it would put audio in the clear on the LAN while the note describing it is encrypted.
- [ ] 9.4 Range-resumable, so an interrupted 40 MB transfer resumes rather than restarts.
- [ ] 9.5 Distinguish **evict** (one device drops bytes, refetchable), **expire** (every replica drops it, not recoverable) and **redact** (every replica, immediately, deliberately unrecoverable). §5.6 says these were conflated in an earlier draft.
- [ ] 9.6 Then hold-to-talk, and then the assistant intent.
