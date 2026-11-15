## Why

Asked for on 2026-11-13: "set an alarm to sunrise or sunset — a wake up alarm
every day at sunrise." The person chose **the phone's location now** as whose
sunrise: the alarm follows them when they travel.

Nothing in the archive can say this today. A series rings at one fixed local
clock time (`rule.rs:202`), an occurrence is named by that computed start, and
the recognizer deliberately refuses `sunset` (`tests/recognize.rs:732`). There
is no notion of where the person is beyond per-intent geofences.

## What Changes

- **The recognizer reads sun times:** "at sunrise", "at sunset", "30 min before
  sunset", "an hour after sunrise", "every day at sunrise", "weekdays at
  sunrise", "tomorrow at sunset". "Watched the sunset" stays a note.
- **A sun-anchored series and a sun-anchored moment** are stored as new kinds of
  time: which sun event, an offset, and the zone — never coordinates.
- **Each occurrence is named by its date**, so moving, the calculation improving,
  or a location change never orphans a done, skip or move.
- **The phone resolves the time.** It keeps its own last known location, rounded
  to about a kilometre, as device-local state that never enters the log or
  sync. The Rust core computes sunrise and sunset from it; Kotlin only hands
  over the fix.
- **The phone rings it**, through the existing alarm path, with the existing
  warning-ahead and through quiet hours (a named time is time-critical).
- **The desktop shows it without a time** — "at sunrise" — because it does not
  know where the person is.
- **No location, no guess.** Without any fix the alarm is written but the phone
  says it cannot ring it until it has a location. A day with no sunrise (polar)
  is skipped and the day says so.

## Capabilities

### New Capabilities

- `solar-time`: sun-anchored moments and series, occurrence identity by date,
  device-local location, resolution, ringing, the no-location and polar cases.

### Modified Capabilities

- `recognizer`: sun phrases are recognized; non-alarm uses of the words are not.

## Impact

- `capture-core`: `When::Solar` and a solar series form (new enum variants;
  older builds hold them as malformed — the phone and desktop update together,
  as with `Promoted::Page`).
- `capture-time`: a `no_std` sunrise/sunset function (NOAA method, `libm`),
  pure of its inputs.
- `capture-parse`: sun vocabulary; `GRAMMAR_VERSION` 9 → 10.
- `capture-store`: occurrence rows for solar series carry a date and anchor,
  no instant; `due_by` / `announce_by` / `next_wake` take an optional location.
- `capture-ffi`: `set_here(lat, lon, fix_ms)`; due and wake use it.
- Android: coarse location permission; a fix pushed on app open, on time-zone
  change, and before each re-arm where the platform allows.
- Desktop: agenda/calendar draw "at sunrise" with no time.

Derives from DESIGN.md §1.3 (schema evolution), §2.4.1 (location cues fire on
the phone), §3.2 (time model), §3.4–3.5 (recurrence, occurrence identity), the
alerts design (alerts derived, not stored; one alarm armed at a time).

## Non-goals

- Dawn and dusk (civil, nautical twilight), solar noon, moon phases.
- A saved "home" place or per-alarm places (considered, not chosen).
- The desktop ringing sun alarms.
- Sun-anchored intents (`Cue::Recurring`); intents still have no recurring
  producer.
