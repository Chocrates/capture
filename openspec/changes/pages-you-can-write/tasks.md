## 1. Pages in the store

- [x] 1.1 Tests first: a page is never promoted by a sweep or an edit; `Promoted::Page` folds and round-trips; reread un-pages it.
- [x] 1.2 `Promoted::Page` (capture-core), descriptor, projection kind `page`; calendar/tasks ignore it.
- [x] 1.3 `pages::create(title, under|of)`, `pages::make(name)`; alias from title where free.
- [x] 1.4 Checklist lines (capture-parse `checklist_of`), projection rows, ticks by item text; tests for reorder and `[x]`.
- [x] 1.5 `completions(sigil, prefix)` over pages, people, projects, tags.

## 2. Surfaces

- [x] 2.1 CLI: `capture page new TITLE [--under NAME | --of @x]`, `capture page NAME`, `capture unpage NAME`; `show` draws checklist lines.
- [x] 2.2 FFI: create/make/unmake page, checklist lines, tick line, completions.
- [x] 2.3 Phone: editor screen with completion; New page (Pages tab, entity page); Make this a page; page drawing with tickable lines and tappable links.
- [x] 2.4 Desktop: New page; Make this a page; completion for all four sigils; checklist drawing.

## 3. Verification

- [x] 3.1 `make ci`; sabotage each.
- [ ] 3.2 Dev app: write a packing page on the phone, tick lines, link it from another page, see it on the desktop after sync.
- [ ] 3.3 Real use for a week.

## Notes from building it

- Starting a page *as* a person's or project's page (D2's `--of @x`) is not built: the store's
  `create` takes a parent only. Binding an existing page to an entity works as before.
- `[[` completion offered a named note twice (alias and slug); `names_like` now gives one offer per
  note, by its chosen name where that matches. The desktop had the same duplicate.
- The phone drew `@`/`^`/`#` as tappable but not `[[links]]`; `pieces` now splits links too.
- The desktop's `names` command is gone; both editors call `completions`.
- 2.3: seen on the dev app 2026-10-14: saved, ticked; a page offered "Everything on #" (fixed, 0778e90).
