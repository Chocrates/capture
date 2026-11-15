## Context

The desktop is three processes: the resident `capture-overlay` (chord, window,
the 60 s due tick that sends notifications), `capture-desktop` (Tauri browse
window), and opt-in `capture-syncd`. The hotkey is chosen by
`hotkey.rs::select` from GNOME gsettings, the portal, X11, or None. Notifications
shell out to `notify-send`. Login start writes an XDG autostart entry, and
`install.sh` writes systemd user units.

The test machine is a managed work Mac. Assume:
- MDM may block unsigned binaries, LaunchAgents, `osascript` or multicast;
- no Accessibility grant is possible;
- nothing can be copied off it.

## Goals / Non-Goals

**Goals:** capture by chord, the browse window and notified alerts work on an
unsigned, locally built Mac install; every failure is visible as a code the
person can read back; nothing in a Mac round trip needs a log.

**Non-Goals:** signing, bundling, macOS CI, alarm-style ringing, Windows.

## Decisions

### D1. Carbon `RegisterEventHotKey` via `global-hotkey`

It is the one global-hotkey API on macOS that needs no privacy grant, so it
survives unsigned rebuilds — the exact failure §8 Platform scope feared. The
`global-hotkey` crate (Tauri project) wraps it. It is added as a
`[target.'cfg(target_os = "macos")'.dependencies]` entry so Linux builds are
unchanged.

`Mechanism::MacCarbon` is selected first on macOS. `--register` waits for one
press and prints Confirmed or Not seen, as today.

Rejected:
- **`CGEventTap` / `NSEvent` global monitor:** needs Accessibility, the grant
  that dies with the signature.
- **Hand-written Carbon FFI:** `unsafe` the crate already gets right.

A system-reserved chord (Spotlight takes ⌘Space) is consumed before the
handler sees it. Registration of a taken chord fails, and the failure is loud
(§8.9).

### D2. The overlay is an accessory app

winit's macOS `ActivationPolicy::Accessory`: no Dock icon, no menu bar. On the
chord it activates the app, raises the window and focuses the field, and hides
on submit or Escape. The X11-forcing path (`window.rs:276`) is already excluded
on macOS.

### D3. Notifications through `osascript`

`notifier::notify` on macOS runs `osascript -e 'display notification … with
title "capture"'`. A non-zero exit or a missing binary means not delivered, and
the moment is retried as `notify-send` failures are.

Notifications are attributed to Script Editor. MDM may disable them; `doctor`
detects that only by asking the person (N-checks below), because macOS gives an
unsigned caller no way to read its notification permission.

Rejected for now: `UNUserNotificationCenter` — it needs a signed bundle.

### D4. LaunchAgents, printed by default

`capture-overlay --autostart` on macOS writes
`~/Library/LaunchAgents/dev.capture.overlay.plist` (`RunAtLoad`, `KeepAlive`
false) and runs `launchctl bootstrap gui/$UID`. `install.sh` prints these unless
`--with-chord`; `--with-sync` does the same for `capture-syncd`. Same rule as
Linux: a change to the person's environment is opt-in.

### D5. `install.sh` on Darwin

`uname -s` branches the script:
- Xcode command-line tools are required (`xcode-select -p`).
- WKWebView is always present, so the desktop app is always built.
- No `pkg-config` check, no systemd.
- The prefix defaults to `~/.local/bin`, with a note to add it to `PATH` in
  zsh (macOS' default shell).

### D6. `capture doctor` speaks in codes

`capture doctor` runs on every platform. Each line is `CODE pass|fail|skip:
reason-class`. Codes are fixed and documented in `docs/doctor-codes.md`. A
reason-class is a short fixed phrase, never free text from the system.

The output never contains note text, paths, user names, host names, addresses
or keys, so it can be read aloud from a work machine. A test enforces this: it
runs doctor against a scratch store seeded with sentinel strings and asserts
none appear.

| Code | Check |
|------|-------|
| B1 | store opens, version matches build |
| B2 | writer lock taken and released |
| B3 | `verify` of the log passes |
| H1 | a hotkey mechanism was selected (which one, by name) |
| H2 | chord registration did not fail |
| H3 | chord was pressed and seen within 14 days |
| O1 | overlay running; O2 socket reachable; O3 starts at login |
| N1 | notifier binary present; N2 a test notification was sent; N3 the person saw it (`doctor --ask` prompts y/n) |
| D1 | desktop app present and launches |
| S1 | sync installed (skip if not); S2 a peer is configured; S3 last round result class |

`docs/macos-test.md` is the round-trip script: what to build, what to run,
which codes to read back, and what each failure means for the next build.

### D7. A Mac compile check from Linux

`make check-macos` runs `cargo check --workspace --target aarch64-apple-darwin`.
`rusqlite`'s bundled SQLite is C, and `cc` needs a macOS SDK, so this may stop
at build scripts. The first task is a spike: try plain `cargo check`, then
`cargo zigbuild`. Keep whichever gets furthest, and record what it cannot
reach. If neither works, the check covers the crates without C dependencies
(core, parse, time).

### D8. Sync off, manual peer

`capture-syncd` builds on macOS unchanged (pure-Rust mDNS, TCP). It is not
installed unless `--with-sync`. `capture peers add HOST:PORT` (new if absent)
records a peer address without discovery. The firewall prompt for a listening
unsigned binary is expected and named in `docs/macos-test.md`.

## Risks / Trade-offs

- [MDM blocks locally built binaries or `osascript`] → doctor shows exactly
  which (B/N codes). Fallback is the CLI, which needs neither.
- [Toolchain install (rustup, Xcode CLT) is not allowed on the work Mac] → Then
  nothing here can be tested until prebuilt binaries come from CI. Ask before
  starting (open question 1).
- [`global-hotkey` pulls dependencies `cargo deny` rejects] → Checked in task
  1; hand-written Carbon FFI as the fallback.
- [A round trip per bug is slow] → D7 catches compile errors here; doctor
  batches every check into one read-back.
- [⌥Space types a non-breaking space in some apps' text] → Registering it takes
  it globally; noted in the docs, and the chord is configurable.

## Open Questions

1. Can rustup and Xcode command-line tools be installed on the work Mac? If not,
   wait for CI-built binaries.
2. Apple silicon or Intel? (Assume `aarch64`.)
3. Does the work Mac allow LaunchAgents in `~/Library/LaunchAgents`?
