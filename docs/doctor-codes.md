# `capture doctor` codes

`capture doctor` prints one line per check:

```
CODE pass|fail|skip: reason
```

Every reason is one of the fixed phrases below. A line never holds note text,
a path, a user or host name, an address or a key, so the output can be read
aloud from a machine nothing may be copied off. `capture doctor --ask` also
asks, on the terminal, whether the test notification appeared (N3).

A failing check is an answer, not an error: doctor always runs every check and
exits 0.

## Store

| Code | Checks | Reasons |
|------|--------|---------|
| B1 | the store opens: log, index, identity | `store opens` · `store does not open` · `no store here` |
| B2 | the writer lock can be taken and released | `taken and released` · `held by another process` · `cannot take the lock` · `no store` |
| B3 | every event verifies (`capture verify`) | `every event checks out` · `damaged events, run capture verify` · `log cannot be read` · `no store` |

**What a failure means.** B1: wrong `CAPTURE_HOME`, or the store is from a newer
build. B2: another capture process is running; try again when it finishes. B3:
run `capture verify` for which events.

## Chord

| Code | Checks | Reasons |
|------|--------|---------|
| H1 | a way to register a global chord was found, and which | `desktop portal` · `gnome keybinding` · `x11 grab` · `carbon hotkey` · `no mechanism on this desktop` |
| H2 | the last registration succeeded | `registered` · `registration failed` · `never registered here` · `no mechanism` |
| H3 | the chord reached the overlay within 14 days | `seen within 14 days` · `not seen for 14 days` · `never seen` · `no mechanism` |

**What a failure means.** H1: this desktop has no supported mechanism; use
`capture + text`. H2: the chord is taken by another application, or the
platform refused; run `capture-overlay --register`. H3 with H2 passing: the
overlay is not running (see O1), or something else consumes the chord first.

## Overlay

| Code | Checks | Reasons |
|------|--------|---------|
| O1 | a `capture-overlay` process is running, for any store (O2 is this store's) | `running` · `not running` · `cannot list processes` |
| O2 | its socket answers | `socket answers` · `socket left behind, nothing answers` · `no socket` |
| O3 | it is set to start at login | `starts at login` · `does not start at login` · `login item written, launchd refused it` |

**What a failure means.** O1/O2: run `capture-overlay --run`. A socket left
behind is cleared by the next overlay to start. O3: `capture-overlay
--autostart`; on a Mac, a fail after that means the LaunchAgent was refused.

## Notifications

| Code | Checks | Reasons |
|------|--------|---------|
| N1 | the notifier is installed (`notify-send`; on a Mac, Capture Alerts.app) | `notifier present` · `notifier missing` · `alert app present` · `no alert app, osascript cannot confirm` |
| N2 | a test notification was sent | `sent` · `refused` · `silenced` · `no notifier` |
| N3 | the person saw it (`--ask` only) | `seen` · `not seen` · `not asked` · `nothing was sent` |

**What a failure means.** N1: install the notifier, or nothing can be said.
On a Mac, `no alert app` means `~/Applications/Capture Alerts.app` is missing
and alerts fall back to `osascript`, which reports sent whether or not
anything appeared; run `install.sh` again. N2 `sent` on a Mac with the app
means Notification Center holds it; `refused` means it is not allowed to post
or was not filed (System Settings → Notifications → Capture Alerts). N3 `not
seen` with N2 `sent`: the alert was filed but not shown, most often a Focus
or Do Not Disturb (allow Capture Alerts under Focus → Allowed Notifications).
`silenced` means `CAPTURE_SILENT` is set.

## Desktop window

| Code | Checks | Reasons |
|------|--------|---------|
| D1 | `capture-desktop` is installed beside `capture` or on `PATH` | `present, not launched` · `not installed` |

## Sync

| Code | Checks | Reasons |
|------|--------|---------|
| S1 | `capture-syncd` is installed | `installed` · `not installed` |
| S2 | a peer is paired or has an address | `a peer is configured` · `no peer` · `sync not installed` · `no store` |
| S3 | the last rounds, as a class | `reaching a peer` · `nothing paired` · `no round has run` · `rounds stopped` · `never reached a peer` · `reaching nobody` · `sync state cannot be read` · `sync not installed` · `no store` |

Sync is never installed on a Mac unless asked for, so S1 `not installed` is
the expected line there.
