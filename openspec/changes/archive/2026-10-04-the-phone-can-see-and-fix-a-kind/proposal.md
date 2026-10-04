## Why

A capture's kind is decided for it — §3.1 makes types promotions rather than
choices, and §3.6 makes time and state the discriminator. That works, and the
timeline now renders what each row is. But the phone can neither *filter* by
kind nor *correct* one.

An undated task has no surface on the phone at all. `Promoted::Task` with
`due: None` reaches no screen: the timeline carries it in capture order among
everything else, and the agenda is a view of time, which is the one thing a
task does not occupy (§3.6). The command line has answered this since `tasks`
and `due` existed. The phone is where capture happens, and it is the surface
that cannot ask what is outstanding.

Correction is worse, because it is the release valve for a deliberate refusal.
The recognizer will not promote below 0.75 confidence, and a task verb alone
scores 40, so "send sarah the migration doc" stays a plain note on purpose —
an honest note beats a confidently wrong label. `browse` already requires that
such a recognition be *offered* rather than applied. But accepting the offer
calls `promote::correct`, and `capture-ffi` exposes no binding for it. The
phone can tick a list item and mark a task done; it cannot say what something
is. So the one surface that sees the misparse first is the one that must wait
until the person is next at a desk.

`browse` already states the principle for the window: every mutation the
command line offers against a note SHALL be reachable there, because a
gradient where suppression needs a shell and destruction is a button is the
wrong way round (§8.4, §8.10). The phone was never held to it.

## What Changes

- The phone gains a **task surface**: what is overdue, what is due, and what is
  open with no date — the three questions `tasks` and `due` already answer on
  the command line, against the same projection queries.
- Open-with-no-date is a first-class band rather than an omission. It is where
  undated tasks have been invisible, and it is most of them.
- `capture-ffi` gains bindings for reclassification — `promote::correct` and
  `promote::as_task` — so the phone can say a note is a task, a task is an
  event, or a promoted thing was never either.
- The note page's existing below-the-floor suggestion becomes **actionable**:
  the offered interpretation can be accepted, which writes a pinned correction.
  Today it is a sentence that names a command the reader cannot run from there.
- A correction from the phone is `pinned: true` like any other, and so beats
  every later reparse, by the rule `promotion` already sets.
- `browse` is extended to hold the phone to the reachability rule it already
  states for the window.

Not breaking. Every change is additive: a new surface, new bindings, and one
existing suggestion line gaining an action.

## Capabilities

### New Capabilities

None. The projection already answers these questions (`tasks_due_on`,
`tasks_overdue`, `due_by`), and `promotion` already defines correction. What is
missing is surface and binding, which belong to an existing capability.

### Modified Capabilities

- `browse`: gains a requirement that a task surface exists and that undated
  tasks are reachable from it; extends "every mutation is reachable" from the
  window to every first-party surface, so reclassification is reachable from
  the phone; makes the below-the-floor suggestion actionable rather than
  informational.

## Impact

- `crates/capture-ffi/src/lib.rs` — new bindings wrapping `promote::correct`
  and `promote::as_task`; regenerated Kotlin binding.
- `android/app/src/main/java/dev/capture/` — a task screen, a route to it, and
  an action on the note page's suggestion line and kind row.
- `crates/capture-store/src/projection.rs` — a query answering tasks with their
  `due` and `done` alongside the descriptor. `promoted("task", n)` returns only
  `(EventId, descriptor)`, which is what the command line renders, and is not
  enough to band rows into overdue / due / undated. `tasks_overdue` and
  `tasks_due_on` both require `due IS NOT NULL`, so neither reaches an undated
  task. No schema change: `promotions` already stores both columns.
- No event kinds, no log format change, no migration. Correction is an ordinary
  `PROMOTE` event and already replicates.

## Non-goals

- **Lowering `CONFIDENCE_FLOOR`.** The floor is why a wrong label is rare, and
  this change exists to make the refusal survivable rather than to weaken it.
- **Asking for a type at capture time.** §3.1 is an invariant: one capture verb,
  zero decisions. Nothing here touches the capture path.
- **Priority, project, effort, sub-tasks or dependencies.** `Markers` parses
  `!p1` and `^project` and stores neither; giving them a home is its own change.
- **Reordering tasks by hand.** `TaskFields.order` exists and nothing writes it.
  Deferred.
- **A desktop task surface.** The window already reaches these through the
  command line's queries; the gap named here is the phone's.
