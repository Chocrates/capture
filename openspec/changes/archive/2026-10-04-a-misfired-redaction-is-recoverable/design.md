## Context

`take_snapshot` writes, `expire_snapshots` expires, `snapshot_count` is
reported. `Ops::snapshot` — the read — has no caller outside tests. There is no
way to list what exists, only to count it.

## Goals / Non-Goals

**Goals.** See what can be recovered. Bring one back. Be honest that it is a
new note.

**Non-Goals.** Resurrecting an event. The phone or the window. Changing the
retention or the cancellation window.

## Decisions

### A new capture, not a resurrection

The body is gone from `events.db` and the redaction is retained so that it
destroys matching events arriving later — including, deliberately, one carrying
the same content from a peer. Writing the old event back would be fought by the
system's own machinery and would be the wrong thing to want.

So recovery writes the text as a new capture: new id, new slug, new position.
The output says so. A command that said "restored" would be claiming the
redaction was undone, which is both false and the more dangerous of the two
possible lies — somebody would stop looking for the other copies.

### Listing needs an accessor that does not exist

`snapshot_count` answers how many. Nothing answers *which*. So a listing
accessor, returning the redaction id, the target, when it was taken, when it
expires and the text — enough for a person to recognise the note they did not
mean to destroy.

### Named by the redaction, as cancelling already is

`capture redact` prints an id prefix and `capture redact --cancel <id>` takes
it. Recovery takes the same prefix, so the two recovery routes inside and
outside the window are named the same way.

### The listing shows the first line only

Enough to recognise it, not a second place the whole content is printed. The
recovery itself prints nothing of the text — it writes a note and names it.

## Risks / Trade-offs

- **It puts destroyed content back within reach of one command.** → It is
  already on disk for thirty days and the status page already says so. A copy
  that cannot be used is not safer, it is only less useful.
- **A recovered note has a different slug.** → Unavoidable, and said. The slug
  derives from the capture, and this is a new one.

## Migration Plan

None. The table exists and is already written.

## Open Questions

- **Should recovery be reachable from the window?** §8.4 says everything should
  be, and redaction's other recovery routes are not there either. One change
  for all of them is better than this one growing a second surface.
