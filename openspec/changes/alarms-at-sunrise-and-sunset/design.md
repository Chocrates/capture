## Context

- Timed things are `Instant { ms, zone }`; all-day things are `FloatingDate`.
- A series stores `RuleSpec` plus a fixed `start_ms`. `capture_time::expand`
  walks local days, applies one local time-of-day and resolves DST.
- An occurrence's identity is its original start (`occ: u64` in the header);
  exceptions are keyed `(tgt, occ)`.
- The fold takes the event set and nothing else. Alerts are derived at due
  time by `due_by` / `announce_by`, which already take `now` and `zone`, and
  Android arms one `AlarmManager` alarm from `next_wake`.
- Location exists only as per-intent geofences on the phone. Kotlin may not do
  date math (`kotlin-decides-nothing.py`).

## Goals / Non-Goals

**Goals:** a daily sunrise alarm that rings on the phone at the right minute
wherever the phone is; nothing about location in the log; nothing silently
wrong.

**Non-Goals:** twilight, saved places, desktop ringing, solar intents.

## Decisions

### D1. Location is device-local state, never an event

The phone stores `here = (lat_e6, lon_e6, fix_ms)` in `ops.db`, rounded in Rust
to 0.01° (~1.1 km; sunrise moves well under a minute over that distance).
`ops.db` is local and never synced.

Rejected: storing the place on the alarm or syncing a "current location". Either
would write a movement history into an append-only, never-forgetting log — the
one thing that cannot be taken back short of redaction.

### D2. The fold knows the date; the due path knows the time

Location is not in the event set, so the fold cannot compute a sun time, and it
must not. The fold produces solar occurrences with a **date and an anchor**
(`Rise|Set`, `offset_min`) and no instant. `due_by(horizon, now, zone, here)`
resolves each to an instant at due time: `sun(date, here, event) + offset`.
That function already takes `now` and `zone`, so taking `here` does not change
its nature.

Without `here` (the desktop, or a phone with no fix), solar occurrences are
listed by date with "at sunrise" and produce no `Due`.

### D3. Occurrence identity is the local date

`occ` for a solar series is `at_local(date, 0, 0, zone)` — local midnight of
the occurrence's date in the series' zone, resolved by the existing DST policy.
It depends only on the rule and the zone, both in the log.

Rejected: identity = computed sunrise instant. A new fix, a travel day or a
better algorithm would rename every occurrence and orphan its done, skip and
move exceptions — on re-fold, silently.

A moved solar occurrence (`occ_move`) becomes an ordinary fixed instant; moving
"tomorrow's sunrise" to 07:30 means 07:30.

### D4. New variants, not trailing fields

- `When::Solar { date: FloatingDate, event: u8, offset_min: i16, tz }` at a new
  index, for one-off moments ("tomorrow at sunset").
- `Promoted::SolarSeries { rule: RuleSpec, event: u8, offset_min: i16, tz,
  first_date, fields: SeriesFields }` at a new index.

Older builds hold these as malformed until updated. Rejected: a trailing
`solar` field on `SeriesFields` — an older build would ignore it and ring the
series at its placeholder `start_ms`, which is silently wrong.

### D5. The sun calculation lives in `capture-time`

`sun(date, lat_e6, lon_e6, event) -> Result<i64 ms UTC, Polar>`: the NOAA
general solar position method (the one behind NOAA's online calculator), f64
through `libm`, `no_std`, standard refraction (−0.833°). Output rounded to the
minute. `Polar::NoRise | NoSet` for days the sun does not cross the horizon.

Floats are computed, never stored or hashed, so the "no floats in ids" rule is
untouched.

Accuracy target: within 1 minute of NOAA's calculator for |lat| ≤ 60°; within 5
minutes up to 66°. Tested against reference values copied from NOAA's
calculator into the test file, with the source and the date copied.

### D6. The phone keeps `here` fresh

- **Permission:** `ACCESS_COARSE_LOCATION` only, asked when the first sun alarm
  is written, with the reason.
- **Fix on app open, on `TIME_ZONE_CHANGED`, and from the existing geofence
  fixes** (`Here.kt`). Kotlin passes raw coordinates; Rust rounds and stores.
- **Before each re-arm** the phone asks for the last known location if it may
  without background permission. Otherwise the stored one is used.
- **Staleness is shown:** "sunrise 06:52 (where you were 3 days ago)" when the
  fix is older than 24 h. It still rings.

Rejected: `ACCESS_BACKGROUND_LOCATION`. It is a heavy grant for a sub-minute
correction. A trip changes the zone, and `TIME_ZONE_CHANGED` already triggers a
fix.

### D7. Sun alarms are time-critical

A sun phrase names a time of day, so it is an instant in the alerts design's
sense. It rings on the `alarms` channel, gets the warning-ahead lead, and
overrides quiet hours as an explicitly armed moment does. A 06:40 sunrise rings.

### D8. Grammar

Accepted: `at sunrise|sunset`; `N (min|minutes|hour|hours|h) before|after
sunrise|sunset`; `sunrise|sunset` after a recurrence head (`every day at
sunrise`, `weekdays at sunset`); after a day word (`tomorrow at sunset`).
Refused: a bare `sunset` without `at`, `before` or `after` ("watched the
sunset", "sunset boulevard"); `>sunset` is still not Sunday. Offsets are capped
at ±6 h; beyond that the recognizer asks, as it does for an unclear hour.

`GRAMMAR_VERSION` goes 9 → 10, so the sweep re-reads unpinned captures. A
backlog note saying "at sunset" will become a sun event. That is the intended
reading, but the re-read count is reported once.

## Risks / Trade-offs

- [The re-read turns old notes into alarms the person forgot] → Sun moments in
  the past do not ring (alerts never ring the past). Future ones are listed in
  the one-time re-read report.
- [No fix on a new phone, alarm silently absent] → Not silent: the phone shows
  "can't ring until it knows where you are" on the alarm and in `status`.
- [Travel without a zone change (same zone, 1000 km west)] → The stale fix is
  shown. Opening the app refreshes it. Accepted.
- [Polar days] → The occurrence is listed as "no sunrise today"; nothing rings,
  no catch-up ring.
- [Algorithm drift between builds renames nothing (D3)] but can move a ring by
  a minute → Accepted.

## Open Questions

- Should the warning-ahead lead apply to a wake alarm? It does by default (D7).
  Revisit after a week of use.
