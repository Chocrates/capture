## 1. The sun calculation (capture-time)

- [ ] 1.1 Copy reference sunrise/sunset times from NOAA's calculator into a test fixture: ≥ 6 cities (equator, ±35°, ±52°, 64°N), 4 dates each including both solstices, with source and date. Then write the failing tests.
- [ ] 1.2 `sun(date, lat_e6, lon_e6, event) -> Result<i64, Polar>`: NOAA method, `libm`, `no_std`; within 1 min to |lat| ≤ 60°, 5 min to 66°.
- [ ] 1.3 Polar cases: 78°N in June (no set) and December (no rise) (**property**: for every lat/date the result is either a time inside that local date or `Polar`, never a panic or NaN).

## 2. Stored shape (capture-core)

- [ ] 2.1 Tests first: golden bytes for `When::Solar` and `Promoted::SolarSeries`; an older decoder holds them as malformed (not dropped).
- [ ] 2.2 Add both variants at new indices; descriptors.

## 3. Recognizer (capture-parse)

- [ ] 3.1 Tests from the recognizer spec, including "watched the sunset", "sunset boulevard", `>sunset`, ±7 h offset asked.
- [ ] 3.2 Sun vocabulary and grammar; `GRAMMAR_VERSION` 10.
- [ ] 3.3 promote.rs maps the new shapes to `When::Solar` / `SolarSeries`.

## 4. Fold and due (capture-store)

- [ ] 4.1 Solar series expand to occurrences with a date + anchor and `occ = at_local(date, 0, 0, zone)` (**property**: occurrence ids independent of any location passed to due).
- [ ] 4.2 Exceptions: done/skip/move keyed by that id; a move becomes a fixed instant.
- [ ] 4.3 `here` in `ops.db`: set (rounded to 0.01° in Rust), read, age; never in an event or sync message (test greps the encoded log and the sync wire after a run).
- [ ] 4.4 `due_by`, `announce_by`, `next_wake` take `Option<Here>`: resolve, skip polar, nothing without `here`; time-critical (quiet hours overridden).
- [ ] 4.5 The one-time re-read report counts notes newly read as sun times.

## 5. Surfaces

- [ ] 5.1 FFI: `set_here(lat, lon, fix_ms)`, `here_age`; due/wake pass it.
- [ ] 5.2 Android: coarse location permission asked when the first sun alarm is written; fix on open, on `TIME_ZONE_CHANGED`, from geofence fixes, last-known before re-arm; `kotlin-decides-nothing` still passes.
- [ ] 5.3 Phone: alarm and day show computed time, stale age, "can't ring until it knows where you are", "no sunrise".
- [ ] 5.4 Desktop and CLI: "at sunrise" / "at sunset" with no time in agenda, calendar, `show`.

## 6. Verification

- [ ] 6.1 `make ci` and `make android` green; sabotage each new test once.
- [ ] 6.2 Dev app only (never the real archive): a sun alarm set two minutes ahead (via offset) rings; a done mark survives a location change; airplane mode with no fix shows the warning.
- [ ] 6.3 Real use: a week of "every day at sunrise"; compare each ring to a weather app's sunrise.
