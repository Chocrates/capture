## Why

Capturing `dentist thursday 2pm` promotes it to an event with a time, draws it
on an agenda and a calendar, and never says anything on Thursday. The same is
true of `call the dentist tomorrow` — a task with a due date — and of every
occurrence a recurring series expands.

The design does not intend this. §2.4.1 rates a missed nudge as the failure
that matters, §2.6 specifies a whole notification budget on the assumption
there are notifications to budget, and the `projection` spec already requires
the due queue to carry "time-based intent deadlines". None of it fires for
anything the ordinary capture path produces.

What *does* fire is narrower than it looks:

- **Intents** — `capture intent at 2026-09-18T14:00 ring the dentist` — fire on
  the desktop, from a slow tick in the resident overlay, with dedup and a
  30-minute re-arm already built.
- **Geofences** fire on the phone.

So an alert requires either a second, explicit verb after the capture, or a
desktop that happens to be running an overlay. The phone — the device that is
actually with you when 2pm arrives — fires no time cue at all.

This is the gap that makes a week of real use unable to answer the question it
exists to answer. A capture system that knows when things are and never
mentions them is being tested as a filing cabinet.

## What Changes

- **A promotion carrying a time becomes alertable, with no second gesture.**
  An event's start, a task's due date, a series occurrence. §3.1's argument is
  that a type is a promotion rather than a choice; an alert that had to be
  armed separately would put the second decision back.
- **The phone schedules and fires time alerts.** `AlarmManager`, exact for
  time-critical cues, inexact for the rest. Today Android fires geofences only.
- **The notification budget of §2.6 is implemented rather than assumed**: quiet
  hours on by default, batched delivery as the default regime with
  time-critical cues immediate, and deduplication across devices.
- **Muting silences alerts**, on the same terms it silences everything else
  (§2.6). This is the escape hatch for a dated capture that was a record rather
  than a reminder.

## Capabilities

### New Capabilities

- `alerts`: what becomes alertable, when it fires, what is said, and what stops
  it.

### Modified Capabilities

- `projection`: the due queue already promises to carry time-based deadlines;
  this adds the alert deadlines of promotions to it, so one mechanism drains
  everything that becomes due.
- `intents`: the notifier rule and local-cue split stand unchanged. What is
  added is that an intent is no longer the only way to be told something at a
  time, and that firing is subject to the budget.

## Impact

**Code.** `capture-store` — `due.rs` (alert deadlines join the queue),
`notifier.rs` (quiet hours, batching, the budget); `capture-ffi` (what is due,
and what was delivered); `android` (`AlarmManager`, a receiver, the
`SCHEDULE_EXACT_ALARM` permission); `apps/capture-overlay` (the existing tick
gains promotions); `apps/capture-cli` (see what would fire, and when).

**A permission.** Exact alarms need `SCHEDULE_EXACT_ALARM` on Android 13+, and
it is user-granted in Settings rather than by a prompt. Asked for only when
something time-critical is first scheduled, with the same argument §2.4.1 makes
for background location: a cue that only works while the app is open is a cue
that never works.

**No schema change to the log.** Whether a notification was delivered is
operational chatter and stays in local state (§2.5.2), where `mark_fired`
already keeps it. Nothing about an alert is a fact about the archive.

## Non-goals

- **Snooze.** A real want, and a second state machine — it needs a stored
  deferral, a new deadline, and a rule for what a snoozed-then-missed alert
  means. Deferred until a week of use says whether it is needed.
- **Per-note alert times.** "Remind me an hour before" is a lead time, and lead
  times want a vocabulary (`!1h`, "an hour before") that §3.1.5 makes a
  deliberate edit. For now an alert fires at the time the capture named.
- **Alerting on anything without a time.** A task with no due date is not
  overdue and never becomes so; §3.6 is explicit that a manufactured backlog is
  the todo-app failure this design refuses.
- **Reworking review scheduling.** Review already has its own cadence (§2.8);
  what changes is only that its nudges travel through the same budget.
- **Choosing a notifier automatically.** §8.9 stands: until the user picks, the
  rule decides and nothing is written.
