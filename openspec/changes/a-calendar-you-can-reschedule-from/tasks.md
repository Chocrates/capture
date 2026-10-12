## 1. An hour without am or pm is asked

- [x] 1.1 Tests first (capture-parse): "every monday at 2" and "on oct 15th at 2" carry `unclear_hour: Some(2)` and no time; "at 2pm", "at 14", "at 0", "at noon" are unaffected; existing "at 7" assertions change to the question.
- [x] 1.2 `clock_after`: 1–12 after a marker is not a time; recognition records the unclear hour; bump the grammar version.
- [x] 1.3 `capture_parse::unclear_hour(text)` → hour and insertion offset; the near-miss offer generalises to a choice.
- [x] 1.4 `mutate::settle_hour(paths, name, am|pm)` inserts the meridiem as an ordinary edit; store test that the series re-promotes to 14:00 and the offer disappears.
- [~] 1.5 Surfaces: review offers it; `capture show` names it; `capture hour NAME am|pm`; FFI `unclear_hour`/`settle_hour`; phone banner and note page; desktop note page.
  - Done and tested: review line, `show` (prose and JSON), `capture hour`, FFI, desktop note page (IPC test), phone note page (builds). `settle_hour` re-promotes before returning — the CLI scenario caught `show` still reading "every Mon" when it only edited. The phone's calendar rows ask too, in 4.2. **Not yet seen on the phone**: wireless adb was off.
- [x] 1.6 DESIGN.md §3.1.5's "every day at 7 → 07:00" example corrected.

## 2. One calendar read

- [x] 2.1 Tests first (capture-store): `calendar(from, to)` over a weekly series, a one-off, a dated task, an all-day item, a moved occurrence (on its new day, with `moved_from`), a skipped one (returned, flagged), and a range past the horizon (refused by name).
- [~] 2.2 `projection::calendar`; the one-day agenda reads through it.
  - Built as `capture_store::calendar::days` (beside the column packing it uses). The FFI's `agenda` is now `calendar(day, day)`, so the phone's day list and grid share one read — and the phone stops drawing skipped occurrences as happening, which the old FFI agenda did. **Not yet moved over:** the CLI's `agenda` (its JSON has today-only overdue/owed fields and a tested shape) and the desktop's `agenda`; the desktop moves in 5.1.
- [x] 2.3 CLI `capture cal [YYYY-MM | --week [DATE]] [--json]` as a view (json.rs pattern), scenarios test.
- [x] 2.4 FFI `calendar(from, to)`.

## 3. Occurrences named and moved by people

- [x] 3.1 Tests first: an occurrence resolves from `NAME DATE`; none/several refused with candidates; ms still accepted.
- [x] 3.2 Move-to-text resolver: a point from the recognizer; a day alone keeps the time of day; a bare hour is refused with the question; the duration is kept.
- [x] 3.3 CLI `move NAME DATE --to TEXT`, `skip`/`unskip`/`done`/`undone NAME DATE`; output names both times.
- [x] 3.4 FFI: `move_occurrence`, `preview_move`, `skip_occurrence`, `restore_occurrence`, `set_occurrence_done`, `split_series`.
  - Writing its test found D7: a skip after a move, and skip → restore → skip, were silent no-ops in the fold. Fixed (later wins, in the fold's total order), projection schema 14.
  - "thursday" counts from the occurrence's own day while it is ahead (D5): the first version moved next Monday's gym to this Thursday, four days before it.
  - `set_occurrence_done` is now the store's; the CLI and the desktop each wrote the event themselves and both now call it.

## 4. Phone

- [ ] 4.1 Agenda: month grid and week strip toggled in place, shared selected day, swipe by unit, Today; marks from `calendar`.
- [ ] 4.2 Day list: moved occurrences labelled with their origin; skipped ones dimmed with Restore.
- [ ] 4.3 Occurrence sheet: Done, Move this one (live preview of the resolved time), Skip this one, Change this and later.
- [ ] 4.4 On the dev app (`dev.capture.dev`): capture the gym and dentist, settle the hour, move Monday's gym to Thursday 10am, skip and restore one, browse a month — screenshots.
- [ ] 4.5 A moved occurrence alerts at its new time and not its old one, on the dev app.

## 5. Desktop

- [ ] 5.1 Month grid beside the day view, from the same read.
- [ ] 5.2 Occurrence actions: move (with preview), skip/restore, change this and later; IPC tests.

## 6. Close

- [ ] 6.1 `make ci` and `make android` green; sabotage each new test once.
- [ ] 6.2 Deploy to the desktop and the real phone; the person tries their own gym and dentist.
