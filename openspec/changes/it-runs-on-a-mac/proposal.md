## Why

Asked for on 2026-11-13: "we need it to work on mac." The only Mac is the
person's work computer. No logs can leave it, and there is no CI yet
(GitHub Actions is "soon", not now). Every Mac test is a round trip through a
person reading a screen, so the work has to be shaped for that.

**This changes DESIGN.md §8 "Platform scope for M1" (~3568).** That section
excluded macOS because a global hotkey needs an Accessibility grant tied to a
code signature, which an unsigned rebuild loses. The way round it is Carbon's
`RegisterEventHotKey`, which needs no Accessibility or Input Monitoring grant at
all. Signing and notarisation stay deferred. The §8.9 hotkey rules still hold:
⌥Space by default, confirmed by pressing it once, loud failure, CLI fallback.

A survey on 2026-11-13 found the code probably compiles on macOS as it stands
(everything Linux-only is pure Rust or a shell-out), but nothing works there:
- the overlay finds no hotkey mechanism;
- `notify-send` is missing, so every alert fails to deliver;
- nothing starts at login;
- `install.sh` assumes systemd and `pkg-config webkit2gtk`.

## What Changes

- **Hotkey:** a macOS mechanism using `RegisterEventHotKey` (through the
  `global-hotkey` crate), behind the existing trait; selected first on macOS.
- **The overlay behaves as a menu-less agent app:** no Dock icon, raised and
  focused on the chord, hidden on submit.
- **Notifications through `osascript`** (`display notification`), with the same
  delivered-or-not contract as `notify-send`.
- **Login start through LaunchAgents** for the overlay and, opt-in, the sync
  daemon. As on Linux, these are printed unless asked for.
- **`install.sh` learns macOS:** `~/.local/bin` or a chosen prefix, no systemd,
  WKWebView instead of webkit2gtk, Xcode command-line tools checked.
- **`capture doctor`:** one coded line per check (`H1 pass`, `N2 fail: osascript
  refused`). The output holds no note text, paths, user or host names, so it can
  be read aloud or retyped. A `docs/macos-test.md` script tells the person what
  to run and which codes to read back.
- **Sync is built but off.** It is installed only with `--with-sync`, with a
  manually entered peer address because work networks usually block mDNS. The
  person may never turn it on; their concern about moving data off a work
  machine is theirs to weigh, and the default never moves anything.
- **A compile check from Linux** (`cargo check` for `aarch64-apple-darwin`) so
  most breakage is caught before a round trip.

## Capabilities

### New Capabilities

- `macos`: the hotkey, overlay, notifications, login start, install, doctor and
  sync-off-by-default behaviours on macOS.

### Modified Capabilities

None at the requirement level. `capture doctor` is specified under `macos` but
runs on every platform.

## Impact

- `crates/capture-store/src/hotkey.rs`: `Mechanism::MacCarbon`.
- `apps/capture-overlay`: macOS backend, activation policy, autostart via
  LaunchAgent (`autostart.rs`).
- `crates/capture-store/src/notifier.rs`: `osascript` path.
- `apps/capture-cli`: `doctor`.
- `install.sh`, `Makefile` (`make check-macos`).
- New dependency `global-hotkey` (macOS-only target dependency); checked by
  `cargo deny`.

Derives from DESIGN.md §8 Platform scope (~3568, changed here), §8.9 hotkey
registration (~3819), §8.7 due work (overlay tick), §2.6 notification delivery.

## Non-goals

- Signing, notarisation, a `.app` bundle, `UNUserNotificationCenter`. They wait
  until there is something to distribute.
- Ringing-until-answered alarms on the Mac. Desktop alerts are notifications on
  Linux too.
- GitHub Actions macOS CI (next, once a remote exists).
- Windows.
- Intel Macs, unless the work Mac turns out to be one (open question).
