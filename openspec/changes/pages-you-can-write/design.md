## Context

A note is already a wiki page in every structural sense: slugs and aliases name
it (§4.3), parents nest it, entities bind it as a backing note, `[[links]]`
resolve with backlinks, concurrent edits are kept as divergent versions rather
than merged. What is missing is the writing: a page cannot be started, cannot
be edited on the phone, and is read for dates on every edit.

## Decisions

### D1. A page is a promotion, pinned: `Promoted::Page`

The person chose "pages are never read". The store already has exactly the
mechanism for a note no parse may touch: a pinned promotion (§6). So a page is a
note whose promotion is `Page`, written pinned. The sweep never re-reads a
pinned note, so an edit to a page changes its text and nothing else; the
calendar and the task list never see it, because they read promotions.

"Make this a page" is that correction made by hand; "Not a page" is the release
added on 2026-10-13 (`promote::reread`), which hands the text back to the
recognizer. No new event kind, no new conflict rule.

Rejected: a page flag on the note, or a separate `page` event. Both would be a
second answer to "what is this note", alongside the promotion, and the two could
disagree.

Cost: `Promoted` is a CBOR enum, and an older build cannot decode a variant it
does not know; it holds the event as malformed until it is updated. Acceptable
for a two-device archive updated together, and stated in the release note.

### D2. A new page is a capture of its title, then named

"New page" captures the title as the note's text — so the slug is made from the
title, as the frozen slug algorithm requires (root capture text) — writes the
`Page` promotion, and sets the alias to the title's slug form when that name is
free (`car-maintenance`). When it is taken, the page keeps its slug and the
surface says the name was in use. Started from a person, project or parent, it
binds or nests in the same step.

### D3. Checklist lines are read from the text, ticked by line

A line whose first non-space characters are `- `, `* `, `[ ]`, `[x]` or `[X]`
is a checklist item. The text after the marker is the item. `[x]` written in
the text means done; a tick made on the page is an item tick (the mechanism
`#list:` items already use), reconciled by item text so reordering lines keeps
ticks. Ticking does not rewrite the text, so a tick is never an edit and never
conflicts with one.

This is the markdown task-list shape on purpose, so markdown later is a
superset rather than a migration. `# ` at the start of a line (with a space) is
left alone now — not a tag (tags have no space) and not yet a heading.

Only in pages. A capture's line items stay what they are today, because a
capture is read by the recognizer and a leading `- ` is not worth a grammar
change there.

### D4. One editor shape, two devices

- Phone: a full-screen editor route, not a dialog. Monospace off, multi-line,
  IME suggestions on (prose), Save in the top bar, back asks before discarding.
- Desktop: the existing in-place textarea.
- Both: completion after `[[`, `@`, `^`, `#` from one store read
  (`completions(prefix, sigil)`), so the two lists cannot differ.

### D5. Concurrent edits stay as they are

Two devices editing one page offline produce two versions and a divergence
note, never a silent merge (§1.4). Pages make this likelier than captures did.
A line-level merge when the edits touch different lines is the obvious next
step and is out of scope here; the divergence view must at least show both
versions in full on the phone.

## Risks

- A capture turned into a page loses its date reading. That is what was asked;
  "Not a page" reverses it.
- Older builds holding `Page` events as malformed (D1).
