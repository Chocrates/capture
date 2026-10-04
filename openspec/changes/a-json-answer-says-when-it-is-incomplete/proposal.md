# a-json-answer-says-when-it-is-incomplete

## Why

"Where any capture has not yet been read, **these commands** SHALL report that
alongside their output, so that an answer derived from incomplete promotion is
never presented as a complete one." The commands are the agenda, tasks and
occurrences.

`capture tasks --json` carries `"unread":N`. `capture agenda --json` carries
nothing: it builds its array and returns at `read.rs:211`, before every one of
the five `say_if_behind` call sites, all of which are on the prose path.

So the same question asked two ways gets a hedged answer in a terminal and a
confident one in a script — and the script is the consumer least able to notice.
A machine reading an agenda cannot tell "nothing on Tuesday" from "nothing on
Tuesday that has been read yet", and the difference matters most exactly when
the backlog is large, which is when a sweep is most likely to be behind.

The prose path already got this right, and the reason is written on
`say_if_behind`: it is "printed after the answer, never instead of it", because
suppressing the answer would make a read fail when the sweep is behind — §8.8's
failure with the sign flipped. The JSON path does neither: it does not fail, and
it does not say.

## What Changes

- `capture agenda --json` answers an object carrying the entries and the unread
  count, the way `tasks --json` already does.
- **BREAKING (shape, for one command):** the agenda's JSON becomes an object
  rather than a bare array. `tasks --json` made exactly this change already, and
  its comment says why: "A consumer reading an empty list as 'no tasks' was
  being told something that might not be true." The agenda has the same problem
  and a day with nothing on it is a far more common answer.
- A test pins it, because the defect is that nothing checked: the requirement
  names three commands and only one of them was asserted.

## Capabilities

### Modified Capabilities

- `cli` — an incomplete answer says so in JSON too, not only in prose.

## Non-goals

- **Not the occurrences command.** The requirement names agenda, tasks *and*
  occurrences, and there is no `occurrences` command — occurrences are reachable
  one day at a time through the agenda. Whether that satisfies "list ...
  occurrences" is a question worth asking separately; it is not answered by
  changing the agenda's shape.
- **No change to the prose path.** It already reports, in the right place, for
  the reason recorded on `say_if_behind`.
- **No new count.** `unread_count` is the one the other command uses, read with
  the same grammar version.
