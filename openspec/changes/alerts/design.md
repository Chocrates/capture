## Context

Three mechanisms exist and none of them covers the ordinary case.

- **Intents** (`Kind::INTENT`) carry a `Cue` — `Time`, `Recurring`, `Location`.
  `capture-overlay`'s slow tick evaluates them, fires through `notify-send`,
  and records delivery locally. Dedup, the 30-minute re-arm and the
  fires-once-vs-repeats distinction are all already written in `notifier.rs`.
- **Geofences** fire on the phone through `Arrival`.
- **The due queue** (`due.rs`) drains redaction deadlines at every process
  startup and on the overlay's tick.

What is missing is the join: a *promotion* is not an intent, so `dentist
thursday 2pm` — an event with a time — is seen by nothing. And Android has no
equivalent of the overlay's tick, so even an explicit time intent is silent on
the phone unless a desktop happens to be awake.

## Goals / Non-Goals

**Goals:**

- A dated capture speaks, with no second gesture.
- The phone speaks for itself.
- §2.6's budget is real rather than assumed.
- One draining mechanism, not two.

**Non-Goals:**

Snooze; per-note lead times; alerting on undated things; reworking review's
cadence; choosing a notifier automatically. Each is argued in the proposal.

## Decisions

### Alerts are derived, not stored

An alertable moment is computed from the projection — `promotions.start_ms`,
`promotions.due`, `occurrences.start_ms` — rather than written into the log
when a promotion happens.

The alternative is an `alert` event per dated capture, and it is wrong twice
over. It would make the log's size a function of how many things have dates,
and it would make correcting a time into a second event that has to find and
supersede the first. Derived, a corrected time simply produces a different
answer on the next evaluation, which is what §3.1.3's additive promotion
already guarantees.

This also gives the mute and done cases for free: both are already in the
projection, so a muted or completed thing stops being returned as due without
anything having to cancel anything.

### Delivery is local state, exactly as it already is for intents

`notifier::mark_fired` keeps an intent's delivery in `ops.db` rather than the
log, because which device buzzed and when is operational chatter (§2.5.2).
Alerts use the same store, keyed by the moment rather than the note — an
occurrence at 9am on Tuesday and the same series at 9am on Wednesday are
different moments of the same note, and keying by note would deliver one of
them.

### Time-critical means a moment was named

§2.6 splits immediate from batched and gives examples — "an alarm, a meeting
starting" — rather than a rule. The rule this takes: **a moment is
time-critical when the capture named a time of day.**

`dentist thursday 2pm` named 2pm, so 2pm is when it matters. `call the dentist
tomorrow` named a day; delivering it at 00:00 would be obeying the letter of a
due date and missing what the person meant. The first is immediate, the second
joins the next batch.

This is also exactly the distinction the time model already draws (§3.2):
an instant versus a floating date. The rule is not a new concept, it is the
existing one asked a different question.

### Quiet hours are a rule until somebody sets them

§8.9 forbids writing a default to the log on the user's behalf, and the
notifier module is built around that. Quiet hours follow it: 22:00–07:00 local,
evaluated locally, written nowhere, reported as a default rather than as a
choice until somebody sets one.

### Android schedules alarms; it does not poll

`AlarmManager` with `setExactAndAllowWhileIdle` for time-critical moments, and
`setAndAllowWhileIdle` for batch deliveries. A `WorkManager` job cannot do this
— its minimum periodic interval is fifteen minutes and doze stretches it
further, so "a meeting starting" would arrive at some point in the following
half hour.

Only the **next** alarm is scheduled, not one per moment. A phone holding a
year of dated captures would otherwise hold a year of alarms, and the platform
caps how many an application may have. Each firing reschedules the next, which
also means a corrected time is picked up without anything having to cancel an
alarm that was already set.

`SCHEDULE_EXACT_ALARM` is user-granted in Settings on Android 13+ rather than
prompted. Asked for at the moment something time-critical is first scheduled,
with the reasoning shown — the same shape §2.4.1 uses for background location,
and for the same reason: a permission asked before anything has explained why
is the one most often refused permanently.

## Risks / Trade-offs

- **Every dated capture now speaks** → That is what was chosen, and mute is the
  escape. The risk is that a week of use produces a stream and the escape is
  discovered too late to enjoy the week. Watched rather than pre-solved:
  batching is the brake §2.6 chose, and whether it is enough is exactly what a
  week answers.

- **A missed moment arrives late, possibly in a crowd** → A phone switched off
  for a day comes back holding a day of moments. Late alerts join a batch
  rather than firing at once, so the crowd is one delivery. Whether that
  delivery is legible or is a wall is an open question below.

- **Exact-alarm permission may be refused** → Then time-critical moments fall
  back to inexact, which means "around 2pm" rather than 2pm. Said plainly on
  the health surface rather than degrading silently: a cue that fires at the
  wrong time is worse than one that says it cannot be sure.

- **Two devices, one archive, one buzz** → Dedup is specified but is genuinely
  hard when the devices are not in contact: neither knows the other fired. The
  window is short and the dedup is best-effort through the same sync that
  carries everything else. §2.4.1 already accepts a duplicate over a miss, so
  the failure direction is the tolerated one.

- **The overlay's tick is the desktop's only prompt delivery** → A desktop with
  no overlay running gets alerts at the next command. Unchanged from today and
  out of scope, but worth stating rather than discovering.

## Open Questions

1. **What does a batch look like when it holds nine things?** Three deliveries
   a day is the evidenced regime, and the evidence says nothing about what one
   delivery containing a day's backlog does to somebody. Wants looking at after
   a week rather than deciding now.

2. **Should a recurring occurrence alert every time?** "Walk the dog daily"
   probably yes; "standup every weekday at 9" probably not, once the habit
   exists. §3.6's whole argument is that nothing accumulates, and an alert that
   keeps arriving after the habit formed is the same manufactured pressure
   wearing a different hat. Left alerting for now, flagged for the week.

3. **Does a late alert need to say how late?** "Due earlier" is specified;
   whether the person needs the hour is unknown until one arrives.
