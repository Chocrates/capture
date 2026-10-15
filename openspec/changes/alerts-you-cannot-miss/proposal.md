## Why

Asked for on 2026-10-13: alerts "need to be louder and more obnoxious", and
"need to fire before the event and on the event". The alert that day fired on
time and Android played its sound once; the person heard it, and it was still
not enough. Missed appointments are the failure this application exists to
prevent.

Today a timed moment says one thing, once, at the moment: one default
notification sound on the `alerts` channel, vibration off. On a phone set to
vibrate or silent it does nothing at all. And at the moment is too late to
leave for anything that is not in the same room.

## What Changes

- **A timed moment also comes due a lead time before it.** Fifteen minutes by
  default, chosen on the device that speaks. The warning is its own moment with
  its own delivery record, so it is said once and the moment itself is still
  said. It is never said once the moment has begun, and a day-only moment (a
  task's due date, an all-day occurrence) has no warning: there is no "before"
  to a day.
- **Timed alerts on the phone are alarm-like.** A new channel plays the alarm
  sound on the alarm stream, vibrates, and repeats until the notification is
  dismissed or opened. Day-only alerts stay on the quieter channel.
- **The lead time is a setting on the phone**, offered as off, 5, 10, 15, 30
  or 60 minutes.

## Impact

- `capture-store`: `Due.ahead` and `Store::announce_by`; the overlay, CLI
  and FFI ask through it.
- `capture-ffi`: `Alert.ahead_min`; `alert_lead_minutes` / `set_alert_lead_minutes`.
- Android: a new `alarms` channel (a channel's sound cannot be changed after it
  is created, so a new id); insistent, alarm-stream notifications for timed
  alerts; the lead time in Settings.
- Desktop: the overlay says the warning too ("in 15 min: …").
