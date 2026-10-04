# Design

## Two commands, no new behaviour

`promote::correct` writes a `PROMOTE` with `pinned: true` and no grammar or
confidence — "a person did this, and recording a version here would make the
sweep think a parser had". `promote::as_task` normalises the date and delegates
to it. Both are tested. The window could not call either.

So this is `ipc::set_kind` and `ipc::as_task` mirroring the phone's bindings,
and two buttons. The useful thinking is about which buttons, and when.

## Which offers appear, and when

| state | offered |
|---|---|
| plain note | Make a task… |
| task | Make a note |
| event or series | Make a note |

"Make a task…" is hidden on something already a task, because the gesture would
do nothing and an action that does nothing teaches that actions might not work.
Its ellipsis is the house convention for a control that asks something — the
same one `Move…` uses.

Demotion is offered on anything promoted, including a series. Demoting a series
needs no rule, which is what separates it from the opposite direction: making
something a series means writing a recurrence, and a free-text prompt for one
is a worse surface than the capture box, which already reads "every weekday at
9". So the window demotes but does not promote to a series, and that asymmetry
is deliberate rather than an omission.

## The due date stays optional

The phone's binding says it: "No due date on this path. Saying 'this is a task'
and saying when it is due are separate decisions, and `as_task` is the one that
takes a day."

So the prompt accepts an empty answer and makes an undated task. That is the
ordinary case rather than a fallback — §3.6's undated band exists because most
tasks arrive without a day, and a prompt that insisted on one would push people
into inventing dates, which is exactly the manufactured pressure §2.7 refuses.

A malformed date is refused by `as_task` before anything is written, rather than
stored and later found by a query that quietly matches nothing.

## Why not reuse `accept_suggestion`

The window already has it, and it looks adjacent: both end in a pinned
promotion. They answer different questions. `accept_suggestion` takes the
reading the recognizer *made* and was not confident enough to apply — there has
to be one for the control to mean anything. These two are a person saying
something the recognizer never proposed, which is the common case for a bare
task verb.

Conflating them would make the control disappear whenever the recognizer had no
opinion, which is precisely when somebody needs it.
