# the-window-can-correct-a-kind

## Why

"Where a surface renders what a capture is, it SHALL offer to change it."

The window's note page renders what a capture is — the promotion's kind and its
descriptor, and a confidence percentage where the recognizer was not sure. Its
action row offers Edit, Tag, Untag, Move, Rename, Pin, Mute and Destroy. None of
them changes the kind.

Half the requirement is met: a below-the-floor suggestion can be accepted, and
a near miss fixed. What cannot be done is the two plain cases —

- **saying a plain note is a task**, which is the ordinary correction, because
  the recognizer refuses to promote a bare task verb and most tasks therefore
  arrive by a person saying so;
- **demoting a promotion back to a note**, which the requirement calls out
  specifically: "a thing wrongly promoted is as common as a thing wrongly left
  alone, and `Promoted::Note` is a promotion like any other."

The phone has both, and the store's `promote::correct` already writes them
pinned, so a correction already beats every later reparse. Only the window
cannot reach it.

## What Changes

- `ipc::set_kind` and `ipc::as_task`, mirroring the bindings the phone has.
- The note page offers **Make a task…** where the capture is not already one,
  taking an optional due date.
- It offers **Make a note** where the capture is promoted, which is demotion.
- Both go through `promote::correct`, so the correction is pinned and the
  captured text is untouched — promotion annotates, it never rewrites.
- Saying a thing is a task and saying when it is due stay separate decisions:
  the due date is optional on that path, exactly as it is on the phone's.

## Capabilities

### Modified Capabilities

- `browse` — a wrong kind is correctable from the window.

## Non-goals

- **No event or series from the window.** Making something a series means
  writing a rule, and a prompt that takes a recurrence in free text is a worse
  surface than `capture` itself — the capture path already reads "every weekday
  at 9". Demoting a series is covered, because that needs no rule.
- **No change to the confidence floor.** The floor is right and this is the
  other half of it: the recognizer declines to guess, and a person decides.
- **No new store behaviour.** `correct` and `as_task` already exist, already pin
  and are already tested. This is the surface that could not reach them.
