## 0. Before writing Mac code

- [x] 0.1 Ask the person design open questions 1–3 (toolchain allowed, chip, LaunchAgents). If the toolchain is not allowed, stop here and wait for CI-built binaries.
  - Answered 2026-11-14: rustup and Xcode CLT may be installed; Apple silicon (`aarch64-apple-darwin`); LaunchAgents unknown, so O3 is what reports it.
- [x] 0.2 Spike D7: `rustup target add aarch64-apple-darwin`; try `cargo check`, then `cargo zigbuild`; record how far each gets; land `make check-macos` with what works.
  - Plain `cargo check --target aarch64-apple-darwin` stops at the first C build script (`objc2-exception-helper`: the system `cc` takes no `-arch`); clang gets further and stops at blake3 without macOS headers. With `zig cc` as the C compiler (`tools/zigcc-macos`, no `cargo-zigbuild` needed) the whole workspace checks, `--all-targets`, Tauri included. Nothing links, so link and framework errors are still found only on the Mac.
- [x] 0.3 `cargo deny` check on `global-hotkey` and its tree (macOS target).
  - `global-hotkey` 0.9.0 (0.10.0 was five days old), macOS-only; it adds `keyboard-types`. `cargo deny check`: advisories, bans, licences, sources all ok.

## 1. Doctor first (useful on Linux today, and the Mac's only feedback channel)

- [x] 1.1 Sentinel test first: doctor output never contains seeded note text, paths, user, host, addresses, keys.
- [x] 1.2 `capture doctor [--ask]` with codes B, H, O, N, D, S; `docs/doctor-codes.md`; Linux implementations of every check.
- [x] 1.3 `make smoke` runs doctor against the scratch store.
  - Reasons are `&'static str` by type, so free text cannot reach a line. H2 reads `chord.registration` in `ops.db`, now written by every registration path. O1 sees an overlay for any store; O2 is this one's socket. Five sabotages, each caught (one only after adding a damaged-log test).

## 2. macOS backends

- [x] 2.1 `Mechanism::MacCarbon` in `hotkey.rs`; selection order on macOS; unit test for selection.
  - `Session::is_macos` (defaults to `cfg!`), checked first. Default chord on a Mac is Option-Space (Cmd-Space is Spotlight's).
- [x] 2.2 Overlay: `global-hotkey` registration, `--register` confirmation, activation policy Accessory, raise/focus/hide.
  - `carbon.rs` via `global-hotkey` 0.9, manager made on the main thread before the window and kept for the process. Accessory policy set through winit's builder. Raise/focus/hide reuse the existing viewport commands; not seen working, only compiled (`make check-macos`).
- [x] 2.3 Notifier: `osascript` path, delivered-or-not contract; test with a stub `osascript` on `PATH`.
  - Text goes to `osascript` as argv, never into the script, so a note cannot run AppleScript. Tested with a stub that records argv and with a refused and a missing program.
- [x] 2.4 Autostart: LaunchAgent plist write/remove, `launchctl bootstrap|bootout`, `--status` reports it.
  - `capture_store::launchd` writes the plist (checked with Python's `plistlib`), `bootout` then `bootstrap gui/<uid>`. `--status` and doctor O3 both require launchd to hold it, so a refused agent is not reported as starting at login.
- [x] 2.5 `capture peers add HOST:PORT` if absent (all platforms).
  - Keyless `given_addresses` in `ops.db`. Each round tries a given address as each member nothing else found; the handshake that succeeds says whose, and `peer_reached` remembers it. A refused connection is not retried under another key. Two tests in `two_devices.rs`.
- [x] 2.6 Doctor's macOS checks (H via MacCarbon, N via osascript, O3 via LaunchAgent).
  - H1 `carbon hotkey`, N via `NOTIFIER`=`osascript`, O3 `login item written, launchd refused it`. All cfg-selected; no separate Mac-only checks needed.

## 3. Install

- [x] 3.1 `install.sh` Darwin branch (D5); `--with-chord` and `--with-sync` write LaunchAgents; plain install writes none (test: run under a fake `uname` and a scratch `HOME`).
  - Also `--with-export` as a daily LaunchAgent. `tests/install_sh.rs` runs the script under stub `uname`/`cargo`/`launchctl` and a scratch `HOME`: plain install writes no agent and calls no `launchctl`. Script honours `CARGO_TARGET_DIR` for that.
- [x] 3.2 `docs/macos-test.md`: build steps, the doctor round-trip script, firewall prompt, what each failure means.

## 4. Round trips on the work Mac

- [ ] 4.1 Round 1: build, `make install`, `capture doctor --ask`; the person reads back the codes; record them here (codes only).
  - 2026-11-16, CI binaries copied by hand: "it mostly works". The test notification was not seen (N3), as `osascript` never worked on that laptop. Exact codes not read back yet. `make install` not tried: it needs cargo, so a download could not use it (fixed in 4.4).
- [ ] 4.2 Fix and repeat until B, H, O, N, D pass. Record each round's codes here.
- [ ] 4.3 Real use for a week of work-day captures; whether to turn sync on stays the person's call.

- [x] 4.4 `install.sh` in the CI download installs what is beside it and builds nothing (no cargo or Xcode tools needed); it clears quarantine and puts `Capture Alerts.app` in `~/Applications`. Test: `a_download_installs_what_it_holds_and_builds_nothing`; two sabotages caught.
- [x] 4.5 `Capture Alerts.app` probe (D9): bundle, ad-hoc signature, `--probe`/`--settings`, codes A1–A7; CI builds and bundles it and runs `--settings` on the runner; `docs/macos-test.md` §3a. Type-checks for aarch64-apple-darwin; not yet run on a Mac.
- [ ] 4.6 The person runs §3a on the work Mac and reads back the A lines.
  - Round 1, 2026-11-18: `open` showed the permission prompt; no log file was left. `--probe`: A1 pass, A2 before allowed, A3 allowed, A4 posted, A5 in Notification Center, A6 allowed / banners / alerts on / sound on, A7 not seen. So a self-signed app may post, and macOS files the alert, but no banner. Suspects: Focus, or the app counting as frontmost when started from a terminal (no `willPresent` delegate). Added the delegate and `--later` (posted 5 s ahead, app gone when it arrives).
  - 2026-11-18, later: Do Not Disturb is stuck on there and the person could not find how to turn it off, which explains A7. A real capture ("onboarding at 10am") showed in the agenda with no alert: alerts still went through `osascript`, and its exit 0 spent the alert.
- [x] 4.8 Alerts on a Mac go through the app when it is installed: `capture-alerts --say <title> <body>` exits 0 only once Notification Center holds the alert, 3 when not allowed; `notifier::notify` routes to it, `osascript` only without it. Doctor N1 says which. Test `a_mac_with_the_alert_app_uses_it_and_believes_only_its_exit`; sabotage caught. Not yet run on the Mac.
- [ ] 4.7 If A7 passes: alerts go through the app, delivery confirmed by the delivered list, `osascript` dropped, doctor N checks moved to it. If not: the overlay draws the alert itself. Either way `osascript`'s exit 0 stops counting as delivered.

## 5. Verification

- [x] 5.1 `make ci` green on Linux (Mac code is cfg-gated; Linux behaviour unchanged); `make check-macos` green as far as it reaches; sabotage each new test once.
  - `make ci` green, `make check-macos` green. Eleven sabotages of the section 2 and 3 code, each caught; the given-address test was hanging under one of them and now has a deadline. CI on GitHub runs macOS for real (`.github/workflows/ci.yml`).
