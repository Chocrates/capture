# the-window-answers-what-is-outstanding

## Why

"Every first-party surface SHALL offer a view of tasks, banded into overdue, due
on or before a horizon, and open with no due date." The window offers none.

The banding exists — `projection::tasks_outstanding` assigns Overdue, Due and
Undated — and has exactly two callers: the phone's binding and `capture tasks`.
The window registers no such command, and its nav is Timeline, Agenda, Wiki,
Lists, Review, Health.

The consequence is sharper than "a missing view". A task occupies no time
(§3.6), so the agenda cannot hold it — and the desktop agenda's due band is fed
by a query requiring `due IS NOT NULL`. **So an undated task reaches no window
surface at all**, except by scrolling a timeline ordered by when things were
captured rather than by whether they are still owed.

That is the ordinary case, not an edge one. The recognizer deliberately refuses
to promote a bare task verb, so most tasks arrive by a person saying so, and a
person saying "send sarah the migration doc" is not saying when.

**And the command line does not meet it either**, which I found by running the
new surface's check rather than reading: completing an overdue task left it in
`capture tasks`, rendered "done · due 2026-09-20". `read::tasks` calls
`store.promoted("task", 500)` — every task promotion ever made, flat. It does
not call `tasks_outstanding` at all. So it lists done tasks, has no bands, and
its `--json` carries only slug and descriptor.

So two of the three first-party surfaces fail the requirement. The phone is the
only one that meets it.

## What Changes

- `ipc::tasks` answers the outstanding tasks with their band, mirroring the
  binding the phone already has.
- A **Tasks** surface in the window: the three bands, in order, each labelled,
  with the undated band present rather than folded into the others.
- A task can be completed from it, through the same `promote` write the command
  line makes — not a second path to the same state.
- Completing one takes it out of the bands. Nothing counts it.
- With nothing outstanding the surface says so plainly, rather than drawing an
  empty list that reads as a failure.
- `capture tasks` reads `tasks_outstanding` too, so it bands its output and
  stops listing finished tasks. Its `--json` gains `band` and `due`, which a
  consumer needs to do anything with the answer.
- A test pins what completing does to the listing, because the defect was that
  nothing checked it: the store's own test asserts `tasks_outstanding` drops a
  finished task, and the command was not asking `tasks_outstanding`.

## Capabilities

### Modified Capabilities

- `browse` — the window answers what is outstanding.

## Non-goals

- **No scoring, no streak, no count of what was finished.** §2.7, and the
  requirement says it: a finished task simply stops being outstanding.
- **No due-date editing from the surface.** Correcting a kind and setting a due
  date are the next window gap and belong with `set_kind`, which the window also
  lacks.
- **No change to the banding.** `tasks_outstanding` is the one implementation
  and all three surfaces read it. The horizon and the overdue boundary are its
  business, and they are property-tested already.
- **No change to what the agenda shows.** A task has no time and does not belong
  there; that is the premise of having this surface at all.
