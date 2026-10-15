## 1. Warnings in the store

- [x] 1.1 Tests first: a warning at start − lead; none when underway, for days, or with lead 0; each delivered on its own.
- [x] 1.2 `Due.ahead`, `Store::announce_by`, `notifier::lead_of(ops)`; overlay, CLI `alerts` and FFI `due` pass through it.

## 2. Surfaces

- [x] 2.1 FFI: `Alert.ahead_min`; `alert_lead_minutes` / `set_alert_lead_minutes`.
- [x] 2.2 Phone: `alarms` channel (alarm sound, USAGE_ALARM, vibration), insistent timed alerts, "in 15 min" text; lead time in Settings.
- [x] 2.3 Desktop overlay: the warning says "in N min".

## 3. Verification

- [x] 3.1 `make ci`; sabotage each new test once.
- [x] 3.2 Dev app: a moment 20 minutes out warns at 15 and rings at 0, loud, on vibrate.
- [ ] 3.3 Real use.

## Notes from building it

- Warnings are made by `Store::announce_by(horizon, now, zone, lead)`, which asks `due_by` a lead
  further ahead; not a filter over `due_by`'s result, which could never see a moment not yet due.
- 3.2 on the dev app 2026-10-14: lead 5 min set in Settings; "alarm test at 7:44pm" armed 19:39:00
  exactly, then 19:44:00; at 19:44 the alarm ringtone played on USAGE_ALARM with vibration.
- Not seen: whether the ring repeated until dismissed (FLAG_INSISTENT); the person was holding the phone.
