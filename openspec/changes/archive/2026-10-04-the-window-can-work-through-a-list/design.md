# Design

## The shape is the bug, not just the wiring

`ipc::list` being unreached reads like an oversight to be fixed by calling it.
It is not: calling it would produce a list nobody could tick.

```rust
pub struct ListLine {          // apps/capture-desktop/src/ipc.rs
    pub text: String,
    pub done: bool,
    pub ord: Option<u32>,
}
```

`tick_item_at(name, ord, done)` resolves `name` to a capture and finds the item
at `ord` *within that capture*. A tag-wide list spans captures, so `ord` alone
is meaningless across it — position 0 exists in every capture under the tag.

The phone's binding has the field and says why, and the store has it too
(`ListEntry.tgt`). Only the window's shape drops it. So this is one line of
struct plus one line of mapping, and the view becomes possible.

That is worth noticing in itself: the IPC layer re-declares a shape the store
already has, and the re-declaration lost a field. `duplicated.py` cannot see it
— it compares function bodies, and these are type definitions.

## Ticking from a gathered list

Each row carries its own `slug` and `ord`, so ticking is
`tick_item_at(row.slug, row.ord, done)` — the same call the note page makes,
with the capture taken from the row instead of from the page.

`ord` is `Option<u32>` because a task that joined the list by carrying its tag
has no place in anybody's written order. Those rows are drawn and not tickable
by position; they are already a task with their own completion. Drawing them
without a checkbox is honest — the checkbox would have nothing to address.

## Saying it is a collection

The phone's comment gives two reasons for the slug and the second is not about
ticking: "to say that what is on screen is not one person's single list".

A list of eleven things gathered from four captures looks exactly like one list
of eleven things, and the difference matters when you edit it — editing "the
list" means editing one of four captures. So the view says how many captures
wrote it when that is more than one. Not a count per row, which would be noise;
one line above the list.

## Deleting `ipc::tick_item`

Ticking by words, as a window command. Its doc comment already says a surface
should use `tick_item_at`, and the window has nowhere to type an item's words —
there is no command palette and no free-text field that resolves to an item.

So it goes. The store function and `capture tick <tag> <item>` both stay:
somebody typing words is exactly what a terminal is for, and the ambiguity the
positional form avoids is reported there as a refusal naming the count.

Deleting rather than allow-listing, because an allow-list entry is a claim that
something is reachable in a way the scan cannot see, and this is not — it is
reachable by nobody.

## The gate arrives green

`tools/unreached.py` found both of these. It lands in this change rather than
its own, wired into `make ci`, because a gate that fails on the day it arrives
is one somebody turns off — and the two things it found are fixed here.

Its four failure modes were each verified by a sabotage confirmed to land
first: an unregistered command, a mistyped call, a computed name, and a stale
allow-list entry. The third matters most for its own sake: a computed name
would make the check silently blind, so it is a failure rather than a thing to
tolerate.

What it cannot see is the phone. `unwired.py` records why — uniffi emits a
symbol containing every method's name, so counting those as callers gives a
check that always passes, "which is indistinguishable from no check while
looking like one". An equivalent for the Kotlin surface needs a different
mechanism and is not this change.
