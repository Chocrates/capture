# a-queued-redaction-withholds-everything

## Why

DESIGN §1.1 withholds a capture's content the moment destruction is **ordered**,
not when it is carried out: the thirty-minute window exists so a misfire can be
cancelled, and for those thirty minutes the content is already supposed to be
unreadable. The projection says so three times in its own comments, and
implements it in two places out of four.

`notes.text` is withheld at read time by `NOTE_COLUMNS`, and alerts by
`ALERTABLE` — both centralised, each carrying a comment recording that a
hand-written copy of this rule "was present in two call sites and absent from
the rest". The full-text index is withheld on the write path, correctly:
`n.text.as_ref().filter(|_| n.redaction_pending.is_none())`.

Forty lines above that, `mentions` and `list_items` are filled from
`if let Some(text) = n.text.as_deref()` with no such filter. That condition is
true for the whole cancellation window, because the fold deliberately leaves
`n.text` as `Some(..)` and records the order in `redaction_pending`. So for
thirty minutes after somebody asks for a note to be destroyed:

- it goes on naming every person, project and tag it mentioned, on each of
  their entity pages;
- its checklist items stay readable in full — "milk", "eggs" — through
  `Store::list` and `items_of`, neither of which goes through `NOTE_COLUMNS`,
  so the read-time withholding never applies to them either;
- its links keep the words inside `[[..]]`, which `resolve_links` extracted from
  the same text, and the target's backlinks name the queued note too.

The comments above both of those loops already state the rule they do not
enforce: *"a note whose content is **withheld** or destroyed must not go on
naming people, or an entity page becomes a way to read what redaction was
supposed to have removed."* "Withheld" is exactly the queued state. The guard
`if let Some(text)` catches only the destroyed one.

Two tests currently assert the leak — `tests/entities.rs:249` and
`tests/checklist.rs:302` — and the comment on one concedes it contradicts the
code comment above the code it tests.

## What Changes

- The write path withholds derived rows from a note with a redaction queued, by
  the same condition the full-text index already uses. `mentions` and
  `list_items` are both inside one `if let`, so one guard covers both.
- `links` joins them. It was found by checking every text-derived fill in the
  function rather than stopping at the two already known, and it had no guard at
  all. `tags` was checked and correctly stays — it is built from `TAG`/`UNTAG`
  events and never from the text, which is the distinction the spec draws
  between a tag and a mention.
- **The `DELETE FROM list_items` moves out of the guarded block.** It currently
  sits inside it, so guarding the block alone would stop the rows being
  *deleted* as well as inserted, and a note that already had items would keep
  them — the fix would appear to work on a fresh note and leak on a real one.
- A capture queued for destruction answers no items through `Store::list` or
  `items_of` either — the two queries that reach items without going through
  `NOTE_COLUMNS`. That falls out of the write-path guard rather than being
  enforced a second time in each query: see `design.md` for why adding the
  predicate there would be a fifth copy of the condition this change exists to
  stop copying. It is where the behaviour is *tested*, because it is how a
  reader meets it.
- The two tests that assert the current behaviour are rewritten to assert the
  rule, with their comments corrected rather than deleted — each is a written
  claim about what the system does, and the claim was wrong.
- The withholding condition is named once rather than written four times. The
  comments on `NOTE_COLUMNS` and `ALERTABLE` both record what happened last
  time this rule was copied by hand.

## Capabilities

### Modified Capabilities

- `projection` — a queued redaction withholds every derived table, not only the
  search index.

## Non-goals

- **No change to the window or to cancellation.** Thirty minutes stays thirty
  minutes, `REDACT_WINDOW_MS` remains the authority, and a cancelled redaction
  restores everything exactly as now. This change is only about what is
  readable while the window is open.
- **No change to the fold.** `n.text` staying `Some(..)` with
  `redaction_pending` set is correct and is what makes cancellation possible —
  the fold reports what the log says and the projection decides what a reader
  may see. Withholding in the fold instead would make the content unrecoverable
  on cancel.
- **No `PROJECTION_SCHEMA` bump.** Queueing a redaction is an event that already
  marks its target as affected, so the note is re-written at the moment the
  order is given and the guard applies then. Rows written before this change
  belong to a window that closes within thirty minutes, after which `purge_note`
  deletes them. Recorded here because "does this need a rebuild" is the question
  to ask on every projection change, and the answer being no is a finding rather
  than an omission.
- **`purge_note` and `occurrences`.** A destroyed *series* keeps its expanded
  occurrence rows, which is the same family of defect through a different
  mechanism — a purge edits in place instead of re-folding. It needs its own
  change, and its own fix to the guard that should have caught it.
