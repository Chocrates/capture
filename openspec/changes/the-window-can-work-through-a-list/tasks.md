# Tasks

## 1. The shape

- [x] 1.1 Add `slug` to the desktop's `ListLine`, filled from `ListEntry.tgt`
      resolved to a name, matching the binding the phone already has.
- [x] 1.2 Note in passing that the IPC layer re-declares a shape the store
      already holds, and the re-declaration lost a field. `duplicated.py`
      compares function bodies and cannot see it.

## 2. The surface

- [x] 2.1 A **Lists** entry in the nav, beside Wiki.
- [x] 2.2 The tags that have items, and the items under the chosen one.
- [x] 2.3 Each item tickable in place, by `(slug, ord)` — the same call the note
      page makes, with the capture taken from the row.
- [x] 2.4 A row with no `ord` — a task that joined by carrying the tag — is
      drawn without a checkbox. It is already a task with its own completion,
      and a checkbox there would have nothing to address.
- [x] 2.5 Where the items come from more than one capture, say so once above
      the list. A gathered list looks exactly like a single one, and the
      difference matters the moment somebody edits it.

## 3. The command that reaches nobody

- [x] 3.1 Delete `ipc::tick_item` and its registration. The store function and
      `capture tick <tag> <item>` stay.
- [x] 3.2 Confirm nothing else referred to it.

## 4. The gate

- [x] 4.1 Land `tools/unreached.py`.
- [x] 4.2 Wire it into `make ci`, with a Makefile comment saying what it is for
      and what `unwired.py` cannot do.
- [x] 4.3 Confirm it passes **after** 2 and 3, not before. It arrives green.
- [x] 4.4 Re-verify its four failure modes against the tree as it then stands,
      each by a sabotage confirmed to land before trusting the result.

      | sabotage | reported |
      |---|---|
      | `ipc::wiki_index` unregistered | a command, registered nowhere |
      | `call("list"` → `call("lst"` | the page calls a command that does not exist |
      | `call("health"` → `call(viewName` | the page builds a command name — the scan is blind |
      | a reason for a command that is gone | the allow-list names commands that no longer exist |

      Clean afterwards: 41 window commands, every one reachable.

## 5. Guards

- [x] 5.1 `make ci`. 78 suites and 10 gates green, including the new one; `deny`
      fails on network only — it cannot reach the RustSec advisory database from
      this sandbox.
- [x] 5.2 Open the window against a **scratch** `CAPTURE_HOME`, never the real
      archive: write a list over two captures under one tag, tick an item from
      the Lists view, confirm it sticks and that the other capture's identical
      item is untouched.

      **Partly.** The window launched against the scratch store and ran, but it
      could not be photographed: this desktop is GNOME on Wayland, `grim` gets
      "compositor doesn't support wlr-screencopy-unstable-v1" and the Shell's
      own D-Bus screenshot answers "Screenshot is not allowed". So the view was
      not seen on screen, and that part is left for somebody sitting at it.

      The behaviour it depends on *was* verified end to end, on the same scratch
      store, through the gesture the view calls:

      ```
      #shop: milk, eggs, bread      shop-milk-eggs-bread-ecsxg9q2tb
      #shop: rye flour, milk        shop-flour-milk-wzkfg5hpgj

      capture list shop   ->  [ ] milk  [ ] eggs  [ ] bread  [ ] rye flour  [ ] milk
      tick item 1 of the first capture
      capture list shop   ->  [x] milk  [ ] eggs  [ ] bread  [ ] rye flour  [ ] milk
      ```

      Five items from two captures, with **two reading "milk"** — the exact case
      the slug exists for. Ticking one left the other alone, which is what
      addressing by `(capture, position)` buys and what ticking by words could
      not do. Scratch store deleted afterwards.
