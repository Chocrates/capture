## 1. What is due

- [x] 1.1 Compute alertable moments from the projection — an event's start, a task's due date, an occurrence's start — rather than storing an alert per dated capture. A corrected time then produces a different answer rather than a second event superseding a first.
- [x] 1.2 Exclude what should not speak: muted notes, completed tasks and occurrences, cancelled occurrences, redacted and withheld notes. All four are already in the projection, so none of them needs anything cancelled.
- [x] 1.3 Answer "what has come due" without delivering any of it. The query is used by three surfaces and by the tests; one that fired as a side effect could not be asked twice.
- [x] 1.4 **Property test**: for any set of captures and any instant, what is due is a subset of what is alertable, and nothing muted, done, cancelled or redacted is ever in it. The exclusions are where a wrong answer is a notification about something somebody deliberately silenced.

## 2. The budget

- [x] 2.1 Time-critical is "the capture named a time of day" — an instant rather than a floating date (§3.2). Immediate; everything else joins a batch.
- [x] 2.2 Quiet hours, 22:00–07:00 local, as a rule evaluated locally and written nowhere (§8.9). Reported as a default rather than a choice until somebody sets one.
- [x] 2.3 An explicitly armed moment inside quiet hours fires anyway. Somebody who set an alarm for 6am meant 6am.
- [x] 2.4 Batch deliveries, three a day, per `EVIDENCE.md` §2.5's fourteen-day field experiment. The default regime, not an overflow behaviour.
- [x] 2.5 Delivery recorded in local state keyed by moment rather than by note — an occurrence at 9am Tuesday and the same series at 9am Wednesday are different moments of one note. **Property test**: firing is idempotent per moment.

## 3. Draining it

- [x] 3.1 Alert delivery joins the startup floor in `Session::open`, beside the due-work drain, rather than getting a scheduler of its own. Consequence found immediately: every test that opens a session against a store holding a dated capture began posting real desktop notifications to whoever ran `make ci`. `CAPTURE_SILENT` is the answer — an environment variable rather than a setting, because it is about this process and not about the archive (§8.9), and useful beyond tests for a machine that should hold an archive and stay quiet.
- [x] 3.2 A moment that elapsed while nothing ran is delivered at the next startup and says it was due earlier, rather than presenting itself as current.
- [x] 3.3 The desktop's existing overlay tick picks up promotions, not only intents.

## 4. Proving it before a surface exists

- [x] 4.1 Against a real store: capture a dated thing, ask what is due before and after its moment, deliver it, ask again. This is the first point the feature is real and it should pass before any notification is posted anywhere.
- [x] 4.2 The exclusions, each against a real store: muted, done, cancelled, redacted, and a time corrected before firing.
- [x] 4.3 The late case: a moment that passed while nothing ran, found and marked late.

## 5. The phone

- [x] 5.1 Two FFI calls, operation-shaped: what is due, and what was delivered. Not a general "read the projection" for Kotlin to interpret — `make android-decides` exists to stop that.
- [x] 5.2 `AlarmManager`: `setExactAndAllowWhileIdle` for time-critical, `setAndAllowWhileIdle` for batches. A `WorkManager` job cannot do this — fifteen-minute minimum, stretched further by doze.
- [x] 5.3 Schedule only the **next** moment, rescheduling on each firing. A year of dated captures is not a year of alarms, and a corrected time is picked up without cancelling anything.
- [x] 5.4 Reschedule on boot (`BOOT_COMPLETED`) — alarms do not survive a restart, and a phone that rebooted overnight would silently stop alerting.
- [x] 5.5 Done, and confirmed on the device once the permission was granted: `window=0 exactAllowReason=permission` where it had been `window=+44s`. Ask for `SCHEDULE_EXACT_ALARM` when something time-critical is first scheduled, with the reason shown. Where it is refused, fall back to inexact and **say so on the health surface** — a cue firing at the wrong time is worse than one admitting it cannot be sure.
- [x] 5.6 The notification says the capture's own text, quotes nothing redacted, and opens the note when tapped.

## 6. The desktop and the command line

- [x] 6.1 The overlay's tick delivers promotions through `notifier::notify`, subject to the same budget.
- [x] 6.2 `capture due` — what will fire and when, without firing it. The debugging surface for "why did nothing happen", which is the question this feature will actually generate.

## 7. Seeing it

- [x] 7.1 Done on a Pixel 8 Pro, and it earned its place twice. Captured `drink water in 1 minute` through the share sheet, closed the app, and the notification arrived by itself. Two bugs only this could have found: **POST_NOTIFICATIONS was never requested** except bundled with the location flow, so an archive full of dated captures could not speak unless somebody had also set up a place cue — the alarm fired, the notification posted, and the platform dropped it as `importance=NONE` with no error anywhere. And **delivery was recorded even when the platform dropped it**, so the moment was spent on a notification nobody could have heard; `say` now returns whether it could, and only a real delivery is written down.
- [ ] 7.2 The doze case: something an hour out, phone untouched, screen off. This is what `setExactAndAllowWhileIdle` exists for and the emulator models it least faithfully.
- [ ] 7.3 A reboot with something scheduled beyond it.

## 8. Closing

- [~] 8.1 `make ci` green. The Android suite on a device is still to run.
- [x] 8.4 Chained alarms proven on hardware: two captures two minutes out, app untouched, both delivered in order — the first firing rescheduled the second, which is the whole of the one-alarm-at-a-time design. Also learned that `adb shell am force-stop` cancels an application's alarms (`Reason=pi_cancelled`), so a test that force-stops is testing the test method rather than the feature.
- [ ] 8.2 Answer open question 1 after a week: what a batch holding nine things actually reads like.
- [ ] 8.3 Answer open question 2 after a week: whether a recurring occurrence should go on alerting once the habit exists, or whether that is §3.6's manufactured pressure wearing a different hat.
