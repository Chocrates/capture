# Design

## The rule, and the four places that decide it

§1.1 withholds content when destruction is **ordered**. The projection decides
that in four places, and got it right in two:

| what | where | correct |
|---|---|---|
| `notes.text` | `NOTE_COLUMNS`, read time, a `CASE` over `redact_queue` | yes |
| alerts | `ALERTABLE`, read time, a `NOT EXISTS` over `redact_queue` | yes |
| the search index | `write_into`, `n.text.as_ref().filter(\|_\| n.redaction_pending.is_none())` | yes |
| mentions, list items | `write_into`, `if let Some(text) = n.text.as_deref()` | **no** |

The two that are right carry comments recording what happened the last time this
was decided per call site: *"a hand-written copy of this was present in two call
sites and absent from the rest, and every query using the copy could read a note
somebody had already asked to destroy."* The same mistake, a third time, in the
same file.

What makes the fourth row wrong is subtle enough to have survived review twice:
`if let Some(text)` **is** a withholding test — for destruction. The fold sets
`n.text` to `None` once the body is gone, so the guard catches a destroyed note
correctly. It cannot catch an *ordered* one, because the fold deliberately keeps
the text and records the order beside it in `redaction_pending`, which is what
makes cancellation able to restore anything. So the guard is right about the
state it was written for and silent about the state §1.1 is actually about.

## One accessor, not one condition per table

`fold::Note` carries `is_redacted()` already, so the shape of the answer exists:
an accessor that turns a question about state into one call. Add the accessor
this rule needs beside it and let every derived table ask it.

(The related accessor the mentions comment declines to use, `text_for_purge`, is
on the projection's `Store` rather than on `Note` — it answers "what does the
purge still have to destroy", which is deliberately the text whether or not a
reader may see it. Worth stating because the two read as opposites and sit a
few lines apart.)

```rust
/// The text a reader may see, which is not the text the note has.
fn readable_text(&self) -> Option<&str>
```

`None` while a destruction is ordered, `None` once it has happened, `Some` only
when somebody may read it. The search index's inline filter becomes a call to
it, so the correct implementation that already exists stops being a fourth
opinion and becomes the definition.

The alternative — adding `.filter(..)` to the mentions guard — fixes today's two
tables and leaves the next derived table free to make the same mistake. This
file has now made it three times, which is the argument for the accessor rather
than the condition.

## The delete has to move, or the fix only works on fresh notes

`DELETE FROM list_items WHERE tgt = ?1` sits **inside** the guarded block. Guard
the block and the delete is skipped too, so:

- a note captured, projected with items, *then* ordered destroyed keeps every
  item row — the rows are never inserted again and never removed;
- a note ordered destroyed before it ever had items looks perfectly withheld.

The second is what a hastily written test would check, and it would pass. So the
delete moves above the guard, where it belongs: clearing what a note had is
unconditional, and only the refilling is withheld.

`mentions` has no such delete in this block — it is cleared per note in the
incremental merge, alongside `tags`, `links` and the rest — so it needs nothing
beyond the guard. Worth stating because the asymmetry is the reason one of these
two tables needs a second edit and the other does not.

## Why the ordered state reaches the write path at all

The guard only helps if a note is re-projected when its destruction is ordered,
rather than at some later refold. It is: queueing a redaction marks its targets
as affected, so the incremental merge re-writes exactly those notes, deleting
their derived rows and refilling them under the new guard. That is what makes
"a note that already named somebody stops" true, and it is the scenario worth
testing, because it is the one that fails if the withholding is only a filter on
the insert.

It is also why no `PROJECTION_SCHEMA` bump is needed. Asking "does this need a
rebuild?" is the right reflex on any projection change; here the answer is no,
for a reason rather than by luck. Any rows written before this change belong to
a window that closes within thirty minutes, and `purge_note` deletes them when
it does.

## The read paths get nothing, deliberately

Two queries answer a capture's items without going through `NOTE_COLUMNS`:
`Store::list` selects `li.item` directly, and `items_of` applies no filter at
all. The obvious move is to add the `redact_queue` predicate to both.

Not doing that. It would be a fifth and sixth copy of the condition this change
exists to stop copying, and the write-path guard makes the rows not exist — so
the predicate would be dead weight that reads like protection. One enforcement
point, and the reader's side is where it gets *tested*: the scenarios are
written as "ask for the items of a queued capture and get none", which is
exactly how a reader experiences it and exactly what regresses if the write
guard is ever removed.

The trade is that a store written by an older binary keeps its rows until its
next refold. That is acceptable here and would not be for a destroyed note: the
ordered state is transient by construction, and the destruction that follows
deletes the rows itself.

## Two tests assert the defect and have to be rewritten, not deleted

`tests/entities.rs:249` asserts a queued note is still on the entity page.
`tests/checklist.rs:302` asserts two items are readable while queued and its
comment claims "nothing leaks". Both are written claims about what the system
does, and both claims were wrong. They are rewritten to assert the rule, and
their comments corrected in place — deleting them would remove the only record
that somebody once believed this was correct, which is the part worth keeping.

## Links are the third table, found by asking the question exhaustively

The rule had been got wrong twice in this function, so the remaining fills were
checked rather than fixed one leak at a time. `links` is the third:

```rust
for (i, l) in n.links.iter().enumerate() {
    "INSERT OR REPLACE INTO links (src, ord, name, resolved) ..."
```

with no guard of any kind. `n.links` comes from `resolve_links(&text, ..)`, which
is `link::extract(text)` — so `links.name` holds the words inside `[[..]]`,
written by the person. A note queued for destruction keeps them, and they are
reachable from the *other* end too: the target's backlinks name the queued note
and the text it linked with.

Destruction itself is already covered, by two separate accidents rather than by
this rule: the fold rebuilds `version_links` from the body, so a destroyed note
has no text to extract from and contributes nothing, and `purge_note` deletes
the rows anyway. Only the ordered window leaks, which is exactly the gap the
other two tables had.

So the accessor earns itself a third time, and `links` joins the guarded block.

## Resolved: `tags` is not in this rule, and that is not an inconsistency

It looked like one. `#vault` in a capture's text appears in `mentions`, which
this change withholds, so a `tags` row surviving reads like the same material
treated two ways.

It is not the same material. `n.tags` is assembled only from `TAG` and `UNTAG`
events — `apply_tag` pushes the tag named in the event body, `apply_untag`
rebuilds the set from the additions this device saw. Nothing anywhere derives a
`tags` row from the note's text. The sigil in the text is read by the mention
scanner, which is a different reader of a different thing, and the spec draws
exactly this line: "a tag is something done to a note, and a mention is part of
what the note says" — which is also why a tag can be retracted by an event of
its own and a mention cannot.

So `tags` survives an ordered destruction correctly: it records what somebody
did to the note, not what the note said. Written down because the overlap is
genuinely confusing on a first read, and the next person to audit this will have
the same moment of doubt.
