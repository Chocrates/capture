# Capture latency

The gesture has to feel like it already happened. This records what that
actually costs on the target machine, what is measured continuously, and what
is not measurable from inside the process.

## What is measured forever

The overlay times every appearance, from **the show request arriving** to **the
first frame drawn**, into a local histogram (`brain_store::latency`). It is
reported by `brain status`:

```
overlay     5 shown, half within 2ms, 95% within 2ms
            (request to first frame — the compositor's share is not visible here)
```

Permanently instrumented rather than measured once, because this degrades
gradually — a font cache change, a slower disk, a compositor update — and a
figure from development says nothing about the machine six months later.

## What was measured once, externally

Measured on the target: Ubuntu 24.04, GNOME 46, Wayland, debug build unless
stated.

| segment | median | p95 | how |
|---|---|---|---|
| show request → first frame | ≤2 ms | ≤2 ms | the histogram above, 29 appearances |
| `--show` client, whole process (release) | 3.2 ms | 3.6 ms | 30 spawns |
| `--show` client, whole process (debug) | 5.7 ms | 6.3 ms | 30 spawns |

So the portion this project controls is **about 5 ms** in a release build:
roughly 3 ms to start the client and reach the socket, under 2 ms for the
overlay to draw. Whatever the end-to-end number turns out to be, that is the
share attributable to code here.

The client is the command the desktop runs when the chord is pressed. Its cost
is process start, dynamic linking, connecting to a unix socket, and exit —
which is why it is a thin client and not the overlay.

## What is not measured, and why

**Physical keypress to visible cursor.** No timer inside this process can see
it. The parts outside are:

- the compositor noticing the key and matching it against a binding
- the settings daemon spawning the command
- (the measured client and overlay portions)
- the compositor compositing the frame
- the display showing it

The first two happen before any of this code runs; the last two after it stops.
Reporting the in-process figure as though it were the whole gesture would be the
most flattering possible lie, which is why `brain status` labels it.

To measure it properly: film the screen and the keyboard together at a known
frame rate — 240 fps gives about 4 ms of resolution — and count frames from key
contact to visible cursor. A phone's slow-motion mode is enough.

**This is deliberately not a CI gate.** The number that matters includes
compositor routing, and a test that gated on the in-process portion would pass
while the gesture felt slow.

## Confirmed on the target

2026-08-25, GNOME 46 on Wayland. `brain-capture --register` bound
`<Control><Super>space` through a gsettings custom keybinding, the chord was
pressed, and the overlay recorded the request arriving — the whole path, on real
hardware, not a simulation of it:

```
Ctrl-Super-Space
  → GNOME matches the binding
  → spawns `brain-capture --show`
  → connects to /run/user/1000/brain-<hash>.sock
  → overlay records the gesture and shows the window
```

The first histogram sample from that press: drawn within 2 ms of the request
arriving. The dead-gesture condition in `brain status` cleared itself, which is
the detection confirming its own signal rather than being asserted to work.

## Status

- In-process portions: measured, and instrumented permanently.
- End-to-end: **not yet measured.** Needs the filming described above, on the
  target machine, once.
