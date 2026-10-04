# Tasks

## 1. The shape

- [ ] 1.1 Add `slug` to the desktop's `ListLine`, filled from `ListEntry.tgt`
      resolved to a name, matching the binding the phone already has.
- [ ] 1.2 Note in passing that the IPC layer re-declares a shape the store
      already holds, and the re-declaration lost a field. `duplicated.py`
      compares function bodies and cannot see it.

## 2. The surface

- [ ] 2.1 A **Lists** entry in the nav, beside Wiki.
- [ ] 2.2 The tags that have items, and the items under the chosen one.
- [ ] 2.3 Each item tickable in place, by `(slug, ord)` — the same call the note
      page makes, with the capture taken from the row.
- [ ] 2.4 A row with no `ord` — a task that joined by carrying the tag — is
      drawn without a checkbox. It is already a task with its own completion,
      and a checkbox there would have nothing to address.
- [ ] 2.5 Where the items come from more than one capture, say so once above
      the list. A gathered list looks exactly like a single one, and the
      difference matters the moment somebody edits it.

## 3. The command that reaches nobody

- [ ] 3.1 Delete `ipc::tick_item` and its registration. The store function and
      `capture tick <tag> <item>` stay.
- [ ] 3.2 Confirm nothing else referred to it.

## 4. The gate

- [ ] 4.1 Land `tools/unreached.py`.
- [ ] 4.2 Wire it into `make ci`, with a Makefile comment saying what it is for
      and what `unwired.py` cannot do.
- [ ] 4.3 Confirm it passes **after** 2 and 3, not before. It arrives green.
- [ ] 4.4 Re-verify its four failure modes against the tree as it then stands,
      each by a sabotage confirmed to land before trusting the result.

## 5. Guards

- [ ] 5.1 `make ci`.
- [ ] 5.2 Open the window against a **scratch** `CAPTURE_HOME`, never the real
      archive: write a list over two captures under one tag, tick an item from
      the Lists view, confirm it sticks and that the other capture's identical
      item is untouched.
