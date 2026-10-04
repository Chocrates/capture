# Tasks

## 1. The command

- [x] 1.1 `ipc::tasks` answering slug, text, descriptor, due and band, mirroring
      the phone's binding field for field.
- [x] 1.2 The band as a value on each row rather than three separate lists, so
      the surface can say "nothing outstanding" without consulting three things.

## 2. The surface

- [x] 2.1 A **Tasks** entry in the nav, beside Agenda — the two answer the
      halves of one question, and §3.6's discriminator is that they differ.
- [x] 2.2 The three bands, in order, each labelled, with the undated band
      present rather than folded in.
- [x] 2.3 A task completable from the row, through the existing
      `set_task_done` — not a second write path to the same state.
- [x] 2.4 Completing one takes it out of the bands, and nothing counts it.
- [x] 2.5 Nothing outstanding says so plainly.
- [x] 2.6 A row opens its note, the way the other surfaces' rows do.

## 2b. The command line, which does not meet it either

- [x] 2b.1 `read::tasks` reads `tasks_outstanding` instead of
      `promoted("task", 500)`, so a finished task leaves the listing.
- [x] 2b.2 Band its output, in the same order the window uses.
- [x] 2b.3 `--json` carries `band` and `due`. A consumer given a descriptor
      string has to parse prose to learn when something is due.
- [x] 2b.4 A CLI test: complete a task and it leaves the listing. The store's
      own test asserted this of `tasks_outstanding` all along — the command was
      not asking `tasks_outstanding`. Verified against the real defect rather
      than a sabotage: reverting `read.rs` to the committed version fails the
      new test with "the overdue band is missing", printing the flat unbanded
      listing underneath.

## 3. Guards

- [x] 3.1 A frontend test: the view exists, reads `tasks`, draws the undated
      band, and completes through `set_task_done`.
- [x] 3.2 Verify it is non-vacuous by breaking each assertion in turn. Three
      sabotages, all caught: dropping the undated band, completing by another
      path than `set_task_done`, and dropping the empty-state message.
- [x] 3.3 `make unreached` — the new command must be reachable, which is the
      check that would have caught this surface being registered and unused.
- [x] 3.4 `make ci`. 78 suites green; `deny` fails on network only.

## 4. Seeing it

- [x] 4.1 On a **scratch** `CAPTURE_HOME`, never the real archive: an overdue
      task, one due later, one undated, confirm each lands in its own band and
      that completing one removes it.

      ```
      overdue
        invoice-gdf6zf0qfp     task · due 2026-09-20   pay the invoice
      due
        call-plumber-…         task · due 2026-12-01   call the plumber
      no date
        send-sarah-migration-… task                    send sarah the migration doc
      ```

      Completing the overdue one dropped it *and* its band heading; completing
      all three gave "Nothing outstanding." `--json` carried
      `"band":"due","due":"2026-12-01"` and `"band":"undated","due":null`.
      Scratch store deleted afterwards.
- [x] 4.2 The window cannot be screenshotted on this machine — GNOME on Wayland
      refuses both `grim` and the Shell's own D-Bus call. Record what was
      verified through the store and what is left for somebody at the screen.

      Verified: the banding and the completion behaviour, through the command
      line, which now reads the same query the window does. Left for somebody at
      the screen: that the three bands are legible and in the right order, and
      that the checkbox is where a hand expects it.
