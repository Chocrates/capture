# Tasks

## 1. The commands

- [x] 1.1 `ipc::set_kind(name, kind)` taking "note" or "task", mirroring the
      phone's binding, including its rule that this path sets no due date.
- [x] 1.2 `ipc::as_task(name, due)` with the due date optional and normalised by
      the store, so a malformed one is refused before anything is written.

## 2. The surface

- [x] 2.1 **Make a task…** on the note page where the capture is not already a
      task, asking for an optional due date.
- [x] 2.2 **Make a note** where the capture is promoted — demotion, which the
      requirement names specifically.
- [x] 2.3 Neither offered where it would do nothing.
- [x] 2.4 Say what happened, and say what was not touched: the text is
      unchanged, because promotion annotates and never rewrites.

## 3. Guards

- [x] 3.1 A frontend test: both controls exist, call the right commands, and
      the due date is optional.
- [x] 3.2 Non-vacuity, by breaking each assertion in turn. Four sabotages, all
      caught — and the fourth found a weak assertion: looking for "The text is
      unchanged" *once* passed with it removed from either control, because the
      other still said it. Counted now, so losing half the reassurance fails.
- [x] 3.3 `make unreached` — both new commands must be reachable.
- [x] 3.4 `make ci`. 78 suites green; `deny` fails on network only.

## 4. Seeing it

- [x] 4.1 On a **scratch** `CAPTURE_HOME`: a plain note made a task appears in
      the undated band; made a task with a date it lands in the right band; a
      promoted capture demoted leaves the bands entirely. Verified through the
      store, since this desktop cannot be screenshotted.

      `send sarah the migration doc` — below the floor, so "Nothing
      outstanding" before. Made a task with no date (what the window's prompt
      does with an empty answer) it appeared under **no date**; given
      `2026-09-20` it moved to **overdue**.

      Demotion could not be exercised from the terminal: there is no verb for
      it. `capture task` promotes and nothing demotes, which is its own small
      finding — the phone and now the window can demote and the terminal
      cannot. The CLI's own requirement is weaker ("correct a promotion ...
      pinned"), which `capture task` meets, so this is a question rather than a
      violation.
- [x] 4.2 Confirm a correction survives a later sweep, which is the half of the
      requirement a surface cannot show on its own.

      `an_edit_does_not_displace_a_correction` already covered promotion. **No
      test demoted anything**, and the obvious assertion there — "it is a note
      now" — passes whether or not the demotion is pinned. So
      `demoting_to_a_note_holds_against_the_recognizer` takes a capture the
      recognizer promotes confidently and correctly, demotes it, edits it to
      force a reparse, and checks it is still a note. Unpinning `correct` fails
      it with "a sweep re-promoted something a person had demoted".
