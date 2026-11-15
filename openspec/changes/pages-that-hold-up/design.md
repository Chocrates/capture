## Context

A page is a note with a pinned `Promoted::Page` (D1, `pages-you-can-write`).
Its text is whole-text `EditBody` versions linked by `prev`; concurrent tips
are kept as divergences, newest HLC shown. Names are slug or alias in one
namespace; `[[name]]` resolves at fold time to what the name meant when the text
was written. Checklist ticks are separate `item_done`/`item_undone` events
matched by item text, then position.

Current tests: 8 in `tests/pages.rs`, checklist and link suites, generic
two-device divergence. No page-specific two-device test, no Android page test.

## Goals / Non-Goals

**Goals:** every scenario in the gap list below has a test; the four defects in
the proposal are fixed; the fixes are safe for every note, not only pages.

**Non-Goals:** line merging, markdown, page size caps, new event kinds.

## Gap list (what gets tested)

| # | Case | Layer | Kind |
|---|------|-------|------|
| G1 | Same page edited on two devices, synced: both versions kept, one divergence, same tip shown on both | two_devices | property (random interleavings of edits and syncs converge) |
| G2 | Divergence on a page resolved on one device; the other sees it resolved after sync | two_devices | example |
| G3 | Tick on device A while device B rewords that line; after sync the tick follows or is reported, never lands on a different line | two_devices | property |
| G4 | `Page` promote crosses sync; a reparse on the other device does not undo it | two_devices | example |
| G5 | Make-page on A races an edit on B | two_devices | example |
| G6 | Alias changed: old `[[old]]` links show broken, not silently retargeted | store | example |
| G7 | Alias freed then taken by another page: earlier links keep their original target | store | example |
| G8 | Forward link written before its page exists reaches the page once it is made (changed 2026-11-15: it stayed unresolved until the linking note was edited, which made a wiki impossible to write in any order; a link that never resolved has no meaning to protect, and links that did resolve keep G6/G7). Link text is also tried as a title, so `[[KV cache maths]]` reaches `kv-cache-maths` | store | example |
| G9 | Titles: accents, CJK, RTL, emoji-only (→ no alias), mixed scripts | store | property over `name_for` |
| G10 | Redacting a page with backlinks and children: linkers show broken, children lose parent, confirmation names them | store + CLI | example |
| G11 | Version history of a page via `capture log` lists every version with origin | CLI | example |
| G12 | Page of 1 MB / 10 k checklist lines: rebuild and search time recorded; a regression budget set from the measurement | store bench | measurement |
| G13 | Search with `"`, `-`, `:`, `*`, `NEAR`, unbalanced parens returns results or nothing, never an error | store | property (arbitrary strings) |
| G14 | Title match ranks above body match | store | example |
| G15 | `--under` with `--of` together: refused or defined, not half-applied | CLI | example |
| G16 | Phone instrumented: new page, edit, tick, link, unpage | androidTest | example |
| G17 | Credential-shaped title and credential pasted by edit | store | example |
| G18 | Two writers on one device: stale `--expect` refused, nothing written | store + CLI | property |

## Decisions

### D1. A flagged title gives no alias

When `looks_like_credential(title)` is true, `pages::create` writes no alias;
`Created.name` is `None` and carries a reason the surface prints ("the title
looks like it holds a credential, so the page keeps a date name").

Rejected: deriving an alias from the non-flagged words. The detector is
heuristic; picking which words are safe is the guess the rule exists to avoid.

### D2. Search quotes every term

The typed query is split on whitespace; each term is wrapped as an FTS5 string
(`"` doubled inside) and the terms are joined with implicit AND. Prefix search
is kept for the last term (`"term"*`) so typing-as-you-search still works.
Ranking uses `bm25(note_fts, w_title, w_body)` with the title column weighted
higher — this needs the title (first line) as its own FTS column, which is a
projection change (new table, never a new column: projection.rs:160) and a
re-fold.

Rejected: exposing FTS syntax as a power feature. Nobody asked for it, and an
agent harness would trip on it constantly.

### D3. The credential flag reads every version

The fold sets `credential` if any version of the text — root or any edit — is
flagged. Clearing the flag (`fold.rs:1222`) stays an explicit act. The flag is
derived state, so this is a re-fold, not an encoding change.

Consequence: a page whose edit is flagged is excluded from export like a flagged
capture. The surface names which version tripped it.

Rejected: checking only the current version. A secret written then edited out
is still in the log and still in every export of history.

### D4. An edit may name what it replaces

`mutate::edit(paths, name, text, expect: Option<VersionId>)`. With `Some(v)`,
the write is refused with `ExitCode::Conflict` (new) unless `v` is the version
the note shows now (its head). A diverged note can still be edited from the
version it shows: that edit loses nothing, and the other tip stays a divergence. With `None`, today's behaviour. The check runs inside the
writer lock (lock.rs), so it is atomic with the append.

`VersionId` is the event id of the version, already shown by `show --json`
(added there if absent) and by `log`.

Rejected: always requiring it. The human editor holds the lock while open and
does not need it; making every caller fetch a version first adds a decision to
editing.

## Risks / Trade-offs

- [D3 flags more notes; some existing pages may become excluded from export] →
  The re-fold reports how many notes changed flag; the release note says so.
- [D2 changes search results people are used to] → Result sets only grow
  (syntax errors become results); ordering changes are intended.
- [G12 may show pages are slow at size] → This change records the number and
  sets a budget; making it faster is follow-up work if needed.
- [G3 may show ticks mis-land after a reword+sync] → Then the fix belongs here,
  before an agent starts rewording pages.

## Open Questions

- Does `--under` with `--of` mean anything? Decide when writing G15; default is
  refuse.
