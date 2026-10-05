# Design

## Which artifact moves, and why this one

The code's comment carries evidence and the spec's sentence carries none. That
is not a rule — a spec is allowed to overrule an implementation, and usually
should — but here the sentence is asserting a general principle and the comment
is recording what happened when that principle was applied to this command.

What happened: `due` opened a session and dropped the result, which satisfies
"does not block on a writer" and produces an answer from stale state. The
failure is invisible, because "nothing is due" looks exactly like "nothing is
due".

So the spec moves, and takes the evidence with it. A requirement that says only
"may settle" invites the next reader to decide it was a shortcut.

## The distinction that does the work

Not *which commands* may take the lock, but *what for*:

- **To discover staleness** — never needs the lock. `unread_count` answers from
  the read-only handle, which is why every other read reports without one.
- **To resolve staleness** — can only be done by writing, because reading a
  capture *is* a write: the sweep records what it read.

§8.8 is about the first. A read that cannot answer until it is caught up is not
a read going via a writer; it is a read that has work to do first.

## Why only `due`

Because only `due` has an answer that is actively misleading while behind.

`tasks` behind is a short list with a line saying so — incomplete and visibly
incomplete. `agenda` behind is the same. `due` behind is most often **empty**,
and an empty answer carries no evidence of its own incompleteness. The staleness
line is doing all the work, and a line is easy to miss when the thing above it
says you have nothing to do.

That is the test for whether another command should join it: not "is the answer
incomplete" but "does the incomplete answer look like a complete one".
