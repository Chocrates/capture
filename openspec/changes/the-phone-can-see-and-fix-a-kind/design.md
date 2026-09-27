## Context

The kind model is finished and correct. `Promoted::{Note, Task, Event, Series}`
lives in a `PROMOTE` event, never on the capture, and a person's correction is
`pinned: true` and beats any reparse. `surfaces-render-the-projection` already
made a row render what it is.

What is missing is entirely on the phone, and in two places:

- **Reading.** `capture-ffi` exposes `timeline`, `note`, `search`, `agenda`,
  `list`. There is no task read. `Promoted::Task` with `due: None` therefore
  reaches no screen: the agenda is time and a task has none.
- **Writing.** `capture-ffi` exposes `set_done` and `tick`. `promote::correct`
  and `promote::as_task` have no binding, so a wrong kind is CLI-only.

The constraint that shapes everything below: **the capture path is untouchable**
(§3.1, one verb, zero decisions). Every affordance here lives on a *reading*
surface, after the fact.

## Goals / Non-Goals

**Goals.** A task surface on the phone with an undated band. Reclassification
reachable where a kind is shown. Banding decided once, in Rust.

**Non-goals.** Lowering `CONFIDENCE_FLOOR`. Any type prompt at capture.
Priority / project / effort / sub-tasks. Hand ordering via `TaskFields.order`.
A desktop task surface.

## Decisions

### Band in the projection, not in Kotlin

`promoted("task", 500)` returns `(EventId, descriptor)` — enough for the command
line, which prints a flat list, and not enough to band. `tasks_overdue` and
`tasks_due_on` both filter `due IS NOT NULL`, so neither can reach an undated
task.

Add one projection query returning each open task with its `due`, `done` and
descriptor, and let the *store* assign the band. The alternative — return rows
and let each surface compare dates — is the mistake
`surfaces-render-the-projection` task 1.1 already corrected for descriptors:
"filled where a store is open … so two surfaces cannot render one note
differently." Overdue is a date comparison in a named zone, and two
implementations of that is two answers on the same day.

*Rejected:* compose `tasks_overdue` + `tasks_due_on` + a third call for the
undated. Three round trips, three chances to disagree about "today", and the
undated arm still needs a new query.

### `done` filters in SQL, not in the UI

A finished task leaves the bands. Filtering in the query keeps the count honest
and avoids shipping rows the surface exists to hide. Nothing counts completions
(§0: nothing scores the user), so there is no "done today" band to keep them for.

### Floating dates stay floating, across the FFI too

`TaskFields.due` is a floating date string — "due Friday" is not 00:00 UTC
Friday (§3.6). The binding carries it as the same `String`, never as an epoch
millisecond. Overdue is computed against the reading zone via the existing
`today()`, not against a UTC instant.

*Rejected:* `due_ms: i64` across the boundary, matching `AgendaEntry`. It would
make a due date an appointment nobody made, which is the exact error §3.6 names.

### Kind crosses the FFI as an enum, not a string

`NoteSummary.kind` is already a `String` (`"note"|"task"|"event"|"series"`) for
rendering. A *write* is different: a typo in a rendering is a cosmetic bug, a
typo in a mutation is a silent no-op or an error at runtime. Reclassification
takes a uniffi enum so Kotlin cannot name a kind that does not exist.

### Corrections live on the note page, not the timeline row

The timeline is a scroll surface. A kind control on a row is a mis-tap away from
silently relabelling something while thumbing past it, and the log would record a
pinned correction that beats every future reparse — a cheap gesture with a
sticky consequence.

The note page already shows the structured half beside the text and already
renders the below-floor suggestion line. Both affordances attach there: accept
the suggestion, or change the kind outright.

### `as_task` and `correct` are both bound, not just `correct`

`correct` takes a `Promoted`, which for a task means constructing `TaskFields`
with a due date. `as_task(name, due)` is the common path and exists in the store
already. Binding both keeps the phone's call sites honest about which one they
mean, and matches the CLI's own split (`args.rs:86` `Task { name, due }`).

## Risks / Trade-offs

- **A pinned correction is sticky by design.** A mis-tap outlives every later
  grammar improvement. → Corrections live on the note page, never on a scroll
  surface; demotion to `Note` is offered on the same terms so a mistake is
  reversible by the same gesture.
- **Two devices correcting the same capture differently.** → Already resolved:
  `promotion/spec.md:31-44` makes pinned beat reparse, and ordinary HLC ordering
  settles pinned-vs-pinned. No new merge rule.
- **The undated band becomes a graveyard.** Most tasks have no date, so this band
  will be the longest, and a long undated list is the thing a task surface is
  supposed to relieve. → Out of scope to solve here; `review` already exists to
  resurface. Worth measuring after it ships rather than pre-solving with ordering
  nobody asked for.
- **`sweep` runs only at session open** (`session.rs:211`). A capture's kind can
  appear a session late, so the task surface can look stale right after a
  capture. → Pre-existing, not introduced here. Noted so it is not rediscovered
  as a bug in this change.
- **A misparse corrected on the phone still needs the desktop to agree.** It does,
  by ordinary sync; nothing is device-local. No mitigation needed, stated so the
  reader does not look for one.

## Migration Plan

None required. Every change is additive:

- No new event kind. A correction is an ordinary `PROMOTE`, which already
  replicates and already folds.
- No schema change. `promotions` already carries `due` and `done`.
- `state.db` is disposable and rebuilt by re-folding, so a new projection query
  needs no migration by construction.

Rollback is removing the surface and the bindings; the log is unaffected either
way, because nothing here invents a record.

## Open Questions

- **Does the task surface get its own route, or a band inside the agenda?** The
  spec requires a view; it does not name a screen. A separate route is the
  assumption here, because folding tasks into the agenda re-conflates time with
  state, which §3.6 exists to separate.
- **What horizon does "due" use?** `due_by` takes one explicitly. The agenda uses
  a day. Overdue / due-today / undated needs no horizon at all, which is the
  simplest reading of the spec and the one taken unless a due-soon band proves
  wanted.
- **Should accepting a below-floor suggestion be one tap or two?** It writes a
  pinned correction, which argues for confirmation; it is also the single most
  common repair, which argues against. Left to implementation, to be answered by
  looking at it.
