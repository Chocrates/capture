## Why

Pages were built in October (`pages-you-can-write`) and are about to carry more
weight: they are what an agent harness will write to next
(`an-agent-keeps-notes-through-the-cli`). The tests cover one device, one writer
and short pages. A survey on 2026-11-13 found the edges untested — two devices
editing one page, ticks racing rewords across sync, renames breaking links,
large pages, non-English titles — and found four defects in code that exists
today:

- **A credential-shaped title leaks through the page's name.** `pages::create`
  sets the alias from the title (`pages.rs:51`) even when the title was flagged,
  so the words §4.3's credential rule keeps out of slugs and exports go into an
  alias that every `[[link]]` repeats.
- **Search passes its query to FTS5 `MATCH` unescaped** (`projection.rs:1974`).
  A quote, a leading `-` or a `:` is an FTS syntax error that reaches the person
  as a raw SQLite message.
- **Credential detection reads only the root capture** (`fold.rs:962`). A secret
  pasted into a page body by an edit is never flagged — and a page is written
  almost entirely by edits.
- **An edit cannot say which version it replaces.** `mutate::edit` sets `prev`
  to whatever head it sees, so two writers on one device (a person and a
  script) overwrite each other silently. Across devices the same race is a
  visible divergence (D5 of `pages-you-can-write`); on one device it is lost
  without a trace, which is the cardinal sin.

## What Changes

- Tests for the gaps listed in the design, at the store layer, across two
  devices (`two_devices.rs`), in CLI scenarios, and on the phone's instrumented
  suite. Several are property tests.
- A flagged title gives a page no alias; the page keeps its date slug and the
  surface says why.
- Search treats what is typed as words, not as FTS syntax. A title match ranks
  above a body match.
- The credential check runs on every version of a note's text, not only the
  first; a flagged edit marks the note as a flagged capture does.
- An edit may name the version it was written against. If the note has moved
  on, the edit is refused and nothing is written.

## Capabilities

### New Capabilities

- `pages`: requirements for how pages behave under concurrency, rename, size and
  redaction. (The `pages` capability is introduced by the unarchived
  `pages-you-can-write`; these are added to it.)

### Modified Capabilities

None at the requirement level beyond `pages`. The search, credential and edit
fixes are written as `pages` requirements because pages are where each one
bites; they apply to every note.

## Impact

- `capture-store`: `pages::create`, `Store::search`, `mutate::edit` (new
  expected-version argument).
- `capture-core`: the fold's credential flag reads every version
  (`fold.rs:962`). State-only; no encoding change, re-fold on upgrade.
- `apps/capture-cli`: `edit --expect` (the flag itself is specified in
  `an-agent-keeps-notes-through-the-cli`; the store half lands here).
- Tests: `crates/capture-store/tests/pages.rs`, `two_devices.rs`, CLI
  `scenarios.rs`, `android/app/src/androidTest`.

Derives from DESIGN.md §1.3 (nothing silently wrong), §3.5 (versions and
divergence), §4.3 (slugs, aliases, the credential rule at ~2433), §6 (pinned
promotions).

## Non-goals

- Merging concurrent edits line by line. Divergence stays the answer (D5).
- Markdown beyond checklist lines.
- A size limit on pages. This change measures cost; it does not cap.
- Soft-delete of pages. Redaction and mute stay the only ways out.
