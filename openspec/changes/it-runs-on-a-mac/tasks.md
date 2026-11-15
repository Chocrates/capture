## 0. Before writing Mac code

- [ ] 0.1 Ask the person design open questions 1–3 (toolchain allowed, chip, LaunchAgents). If the toolchain is not allowed, stop here and wait for CI-built binaries.
- [ ] 0.2 Spike D7: `rustup target add aarch64-apple-darwin`; try `cargo check`, then `cargo zigbuild`; record how far each gets; land `make check-macos` with what works.
- [ ] 0.3 `cargo deny` check on `global-hotkey` and its tree (macOS target).

## 1. Doctor first (useful on Linux today, and the Mac's only feedback channel)

- [ ] 1.1 Sentinel test first: doctor output never contains seeded note text, paths, user, host, addresses, keys.
- [ ] 1.2 `capture doctor [--ask]` with codes B, H, O, N, D, S; `docs/doctor-codes.md`; Linux implementations of every check.
- [ ] 1.3 `make smoke` runs doctor against the scratch store.

## 2. macOS backends

- [ ] 2.1 `Mechanism::MacCarbon` in `hotkey.rs`; selection order on macOS; unit test for selection.
- [ ] 2.2 Overlay: `global-hotkey` registration, `--register` confirmation, activation policy Accessory, raise/focus/hide.
- [ ] 2.3 Notifier: `osascript` path, delivered-or-not contract; test with a stub `osascript` on `PATH`.
- [ ] 2.4 Autostart: LaunchAgent plist write/remove, `launchctl bootstrap|bootout`, `--status` reports it.
- [ ] 2.5 `capture peers add HOST:PORT` if absent (all platforms).
- [ ] 2.6 Doctor's macOS checks (H via MacCarbon, N via osascript, O3 via LaunchAgent).

## 3. Install

- [ ] 3.1 `install.sh` Darwin branch (D5); `--with-chord` and `--with-sync` write LaunchAgents; plain install writes none (test: run under a fake `uname` and a scratch `HOME`).
- [ ] 3.2 `docs/macos-test.md`: build steps, the doctor round-trip script, firewall prompt, what each failure means.

## 4. Round trips on the work Mac

- [ ] 4.1 Round 1: build, `make install`, `capture doctor --ask`; the person reads back the codes; record them here (codes only).
- [ ] 4.2 Fix and repeat until B, H, O, N, D pass. Record each round's codes here.
- [ ] 4.3 Real use for a week of work-day captures; whether to turn sync on stays the person's call.

## 5. Verification

- [ ] 5.1 `make ci` green on Linux (Mac code is cfg-gated; Linux behaviour unchanged); `make check-macos` green as far as it reaches; sabotage each new test once.
