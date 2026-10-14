## Why

Asked for on 2026-10-13 as "the last big piece to make this usable". Capture,
tasks and the calendar now work end to end; writing does not. Every note can
already be a wiki page — a chosen name, a parent, the page of a person or
project — but there is nowhere to *write* one:

- The phone's Edit is a one-line dialog. A page longer than a sentence cannot be
  written or revised there, and it has no `[[` completion.
- There is no way to start a page. A page is a capture, so it begins as whatever
  was typed in the capture box and is named after those words.
- A page is read for dates like any capture. A "gym routine" page saying "every
  monday at 2pm" would itself become a repeating event, and every edit to it
  re-reads the whole text.
- Text is drawn as typed. Links and mentions are followable on the desktop, but
  there are no checklist lines; the only list is the comma form on one line.

The person wants four kinds of page — living reference docs, pages for people
and projects, editable checklists, and captures grown into fuller notes — and
to write them on the phone and the desktop equally.

## What Changes

- **A page is a kind of note that is never read for dates or types.** Made by
  "New page" or by "Make this a page" on any capture. It is never promoted to a
  task, event or series, and editing it never re-reads it; the calendar never
  shows it. A page can be turned back into a capture.
- **New page**: asks for a title. The title is the first line and gives the
  page its name (`[[car-maintenance]]` where free); it can be started under a
  parent page or as the page of a `@person`/`^project`.
- **Make this a page**: on any capture's page. The text stays, the capture
  becomes a page, and the editor opens on it.
- **A real editor on both devices**: full screen on the phone, in place on the
  desktop; multi-line; completion for `[[` pages, `@` people, `^` projects and
  `#` tags; saves a new version, the old one kept.
- **Pages are drawn in the capture grammar**: line breaks kept; `[[links]]`,
  `@people`, `^projects`, `#tags` followable; and **checklist lines** — a line
  starting `- ` or `[ ]` (`[x]` ticked) is an item ticked on the page, by line,
  the way list items are ticked today. Markdown (headings, emphasis) is later
  and is not designed in here beyond not colliding with it.

## Capabilities

### New Capabilities

- `pages`: what a page is, how one starts, that it is never read, checklist
  lines.

### Modified Capabilities

- `browse`: the editor, completion, and how a page is drawn.

## Impact

- `capture-core`: `Promoted::Page` (a new variant; older builds hold such an
  event as malformed until updated — both devices update together).
- `capture-store`: page creation, make/unmake page, checklist lines and their
  ticks; the sweep skips pages.
- `capture-parse`: checklist lines (`- `, `[ ]`, `[x]`).
- FFI, phone (editor screen, New page, completion), desktop (New page,
  completion for all four sigils, checklist drawing), CLI (`capture page new
  TITLE`, `capture page NAME`, `capture unpage NAME`).
