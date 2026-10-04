# the-window-can-work-through-a-list

## Why

A grocery list spread over three captures is readable on the phone
(`ListScreen`) and in the terminal (`capture list <tag>`) and nowhere in the
window. DESIGN §8.4 says everything is reachable from the window.

`ipc::list` exists, is registered in `generate_handler!`, and nothing calls it.
The nav offers Timeline, Agenda, Wiki, Review and Health. The note page draws a
*single capture's* items, which is a different thing — the whole point of a
collection is that it gathers what several captures wrote under one tag.

It is worse than unreached. Even called, it could not work: the desktop's
`ListLine` is `{ text, done, ord }`, and the phone's carries a fourth field
whose comment says exactly why —

> `slug`: the capture this line came from. A collection gathers items from
> however many captures wrote them, and a surface needs to know which — to tick
> the right one by position, and to say that what is on screen is not one
> person's single list.

`tick_item_at` needs the capture and a position. Without the slug the window
could draw a tag-wide list and tick nothing on it.

`ipc::tick_item` is the other half of the same hole: ticking by words, exposed
as a window command, in a surface with nowhere to type words. Its own doc
comment says "a surface should use `tick_item_at`, which cannot be ambiguous".

## What Changes

- `ipc::list` answers the slug alongside the text, matching the binding the
  phone already has. One shape for one question.
- A **Lists** surface in the window: the tags that have items, and the items
  under the chosen one, each tickable in place by position.
- Where a list gathers items from more than one capture, the surface says so.
  That is the second half of the phone's comment, and without it a collection
  silently reads as one person's single list.
- `ipc::tick_item` is **deleted**. The store function and the CLI keep theirs —
  somebody typing an item's words is a real gesture in a terminal. As a window
  command it was reachable by nobody.
- `tools/unreached.py` lands with this change and is wired into `make ci`. It
  is what found both of these, and it arrives green rather than red, because a
  gate that fails the day it lands is a gate somebody turns off.

## Capabilities

### Modified Capabilities

- `browse` — a list is readable and completable in the window.

## Non-goals

- **Not the rest of the window's backlog.** The window is also missing a tasks
  view, a way to correct a capture's kind, and in-row ticking outside the note
  page. Those are three more changes; this one is the list, because it is the
  one with a registered command that cannot work.
- **No change to ticking by position.** `tick_item_at` is the right primitive
  and the note page already uses it. This gives the tag-wide view the same one.
- **No new store query.** `Store::list` already answers what is needed,
  including the `tgt` each line came from; only the IPC shape drops it.
- **Not `make ci` for the phone.** An equivalent gate for the uniffi surface is
  worth having and is a different mechanism — uniffi emits a symbol per method,
  so the trick this gate uses does not transfer.
