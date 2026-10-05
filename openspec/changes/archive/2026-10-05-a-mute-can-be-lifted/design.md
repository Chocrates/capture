## Context

The proposal's claim was that this change lives entirely at the argument layer,
because `MuteBody::until` is folded correctly and threaded all the way to
`write::mute`, which only ever receives a hardcoded `None`.

That claim was half right, and the half that was wrong is the reason this
document exists. Auditing every read of `muted_until` in the projection found
**ten** of them. Seven ask the right question:

```sql
n.muted_until IS NULL OR (n.muted_until != 0 AND n.muted_until <= ?now)
```

Three ask only `n.muted_until IS NULL`, which treats a mute that has *ended* as
still in force:

- `review_eligible()` (projection.rs:733) — the predicate deciding what may be
  offered in review.
- `distinct_mentions()`, both branches (projection.rs:2693, 2697) — the index
  behind the desktop's tag sidebar.

So a mute with an end never un-suppresses from either, and that is already a
live defect rather than one this change would introduce: `mute_entity` takes an
`until` today and `crates/capture-store/tests/entities.rs:291` passes one. It has
gone unnoticed because no *command* could set an end, so the only mutes in any
real archive are indefinite, where the two forms agree.

It matters most for review, because review is the surface §2.6 names first:
"`mute { tgt }` removes a note from **review, resurfacing, and all pushed
surfaces**, permanently or until a stated date." A lapsing mute that never
lapses out of review fails the requirement at its primary site.

## Goals / Non-Goals

**Goals:**

- `capture unmute <note|#tag|@person|^project>` lifts a mute, satisfying
  `specs/review/spec.md`'s "the mute is reversible" and §2.6's "reversible from
  any device, at any time" — for one device.
- `capture mute <target> until <YYYY-MM-DD>` reaches the end-date half of §2.6
  that no syntax could express.
- All three predicates that read `muted_until` agree about what a mute is, so a
  mute lapses everywhere or nowhere.

**Non-Goals:**

- The desktop and phone controls (proposal's Non-goals).
- Unifying `mute_note`, `mute_scope` and `mute_entity` into one store entry
  point. `mute_note` keeps its signature and keeps writing indefinite mutes;
  the CLI routes note mutes through `mute_scope`, which already handles
  `MuteScope::Note` correctly including naming the note so `capture log` sees
  the event.
- Any new persisted state. No table, no column, no `PROJECTION_SCHEMA` bump.

## Decisions

### An unmute is a mute whose end is now, not a new event kind

`MUTE` carries `until: Option<u64>`; `apply_mutes` computes
`Some(until.unwrap_or(0))` and applies mutes in timestamp order so the newest one
touching a note wins; the projection reads an elapsed end as not muted. Writing
`until = Some(now)` therefore reads as unmuted on every subsequent read, on
every replica, with no new machinery.

**Alternative considered: a new `UNMUTE` kind.** Rejected on two grounds. It
would need a fold arm, and more importantly it would need a *rule* for what an
unmute means against the three coarse scopes — does unmuting a note override a
tag mute still in force? "Newest mute wins" already answers that, consistently,
and a second mechanism would have to answer it again and could answer it
differently. §1.3 makes a new event kind the expensive choice, since the archive
keeps it forever.

**Alternative considered: `until = Some(0)`.** Rejected: zero is the sentinel for
*indefinite*, so this would mute harder rather than unmute.

### The three disagreeing predicates are brought into line, not worked around

`review_eligible()` already receives `now` as `?1` for the backoff ladder, so it
needs no signature change — only the predicate. `distinct_mentions` has no `now`
and gains one, which changes its three test call sites and the one real caller
(`ipc.rs:866`).

**Alternative considered: leave them and document it.** Rejected. It would mean
`capture unmute` silently not restoring review, which is the one surface §2.6
names first — an unmute that leaves a note invisible to review is not a
reversal.

**Alternative considered: a shared SQL fragment, so there is one copy of the
predicate.** Attractive, and deferred: `tools/duplicated.py` exists for exactly
this shape, and ten copies of a predicate is what it is for. But that is a
refactor across ten queries, and doing it inside a behaviour fix would make the
behaviour change unreviewable. Recorded for follow-up rather than folded in.

### `until` is a date, resolved to the start of that day locally

A mute lasts in whole days because that is how somebody says it ("not until
Monday"), and a date is what §2.6's "until a stated date" asks for. The instant
stored is the start of that day in the writer's zone, via
`capture_time::at_local`, which is the same conversion the recognizer uses for
an all-day date.

A date already past is **refused**, naming `capture unmute` as what was probably
meant. Accepting it would be an unmute with a confusing spelling, and two ways
to say one thing is the shape `tools/duplicated.py` warns about at the surface.

### Refusal is scoped to what the projection can actually answer

For a note, "is it muted" is a field, so unmuting one that is not muted is
refused. For a tag, entity or date range there is no such record: a coarse mute
is stored only as its effect on each note it covers, and nothing distinguishes a
note suppressed by a tag mute from one suppressed by its own. There was a
`mutes` table once; it was removed earlier because nothing read or wrote it.

So those three scopes are accepted unconditionally, and the message says what was
written rather than claiming something changed. Inventing a scope-level record to
support a refusal would add persisted state to improve an error message, which is
the wrong trade.

## Risks / Trade-offs

**A replica that has not seen the unmute keeps the note suppressed** → Ordinary
convergence, identical to how a timed mute already behaves, and the unmute is an
event that travels like any other. Not mitigated because it is not a fault.

**Changing `review_eligible` changes what comes up for review in existing
archives** → Only for notes carrying a mute with an end, which no command could
create, so in practice only archives written by tests. Stated rather than
mitigated, because the alternative is keeping a predicate that contradicts
§2.6.

**`distinct_mentions` gaining a parameter touches its callers** → Four sites,
one of them real. Compiler-enforced; nothing can silently keep the old
behaviour.

**Ten copies of one predicate, now ten copies that agree** → The duplication is
not reduced by this change, only made consistent. A disagreement can reappear the
next time a query is added. Mitigated by recording the consolidation as
follow-up, and by the fact that `tools/duplicated.py` is the gate that would be
taught to see it.

## Migration Plan

None required. No schema change, no stored-data reinterpretation, and a rebuild
from the log produces the same projection — which is the test for whether a
change needs migration at all (§1.3).

Rollback is reverting the commit; events written by `unmute` remain valid `MUTE`
events and an older binary folds them correctly, because `until` has always been
read.

## Open Questions

- **Should `show` render a mute's end in the reader's zone rather than the
  note's?** It currently uses `n.tz`, the zone the note was captured in. A mute
  is not a property of that zone. Same class as the `intents` UTC bug fixed
  earlier today, but a smaller error — a real zone someone used, rather than one
  nobody did. Left alone here to keep this change to the mute's reversibility.
- **Consolidating the ten `muted_until` predicates.** Noted under Decisions;
  wants its own change.
