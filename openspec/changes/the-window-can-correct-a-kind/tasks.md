# Tasks

## 1. The commands

- [ ] 1.1 `ipc::set_kind(name, kind)` taking "note" or "task", mirroring the
      phone's binding, including its rule that this path sets no due date.
- [ ] 1.2 `ipc::as_task(name, due)` with the due date optional and normalised by
      the store, so a malformed one is refused before anything is written.

## 2. The surface

- [ ] 2.1 **Make a task…** on the note page where the capture is not already a
      task, asking for an optional due date.
- [ ] 2.2 **Make a note** where the capture is promoted — demotion, which the
      requirement names specifically.
- [ ] 2.3 Neither offered where it would do nothing.
- [ ] 2.4 Say what happened, and say what was not touched: the text is
      unchanged, because promotion annotates and never rewrites.

## 3. Guards

- [ ] 3.1 A frontend test: both controls exist, call the right commands, and
      the due date is optional.
- [ ] 3.2 Non-vacuity, by breaking each assertion in turn.
- [ ] 3.3 `make unreached` — both new commands must be reachable.
- [ ] 3.4 `make ci`.

## 4. Seeing it

- [ ] 4.1 On a **scratch** `CAPTURE_HOME`: a plain note made a task appears in
      the undated band; made a task with a date it lands in the right band; a
      promoted capture demoted leaves the bands entirely. Verified through the
      store, since this desktop cannot be screenshotted.
- [ ] 4.2 Confirm a correction survives a later sweep, which is the half of the
      requirement a surface cannot show on its own.
