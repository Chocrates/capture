# Tasks

## 1. Pin the leak before fixing it

- [x] 1.1 Add a failing test: a note naming `@sarah`, destruction ordered, the
      window not elapsed — it contributes no mentions and is absent from the
      entity page. Confirm it fails, and fails by finding the mention rather
      than by panicking elsewhere.
- [x] 1.2 Add a failing test: a capture with items, destruction ordered — asking
      for its items by name answers none, and reading the list by its tag omits
      them while leaving another capture's items under the same tag alone.
- [x] 1.3 Add the test the obvious fix would pass while still leaking: a note
      whose mentions and items were **already projected**, destruction ordered
      afterwards. The rows have to be removed, not merely left unrecomputed.

## 2. One accessor rather than one condition per table

- [x] 2.1 Add `readable_text()` to `fold::Note`: the text a reader may see,
      which is `None` while a destruction is ordered and `None` once it has
      happened. Document it against `text_for_purge`, the accessor beside it
      that answers a different question.
- [x] 2.2 Use it for the mentions and list-items block in `write_into`, in place
      of `if let Some(text) = n.text.as_deref()`.
- [x] 2.3 Use it for the search index too, replacing the inline
      `.filter(|_| n.redaction_pending.is_none())`. The correct implementation
      that already existed becomes the definition rather than a fourth opinion.
- [x] 2.4 Use it for `links` too, which had no guard of any kind. Write the
      failing test first: a note whose text holds a `[[link]]`, destruction
      ordered — the link's name is gone, and the target's backlinks no longer
      name it.
- [x] 2.5 Correct the two comments that state the rule they did not enforce —
      both say "withheld or destroyed" and only destruction was handled.

## 3. Move the delete out of the guard

- [x] 3.1 Move `DELETE FROM list_items WHERE tgt = ?1` above the guarded block.
      Clearing what a note had is unconditional; only the refilling is withheld.
- [x] 3.2 Confirm 1.3 passes, and confirm it *fails* if the delete is left
      inside the guard — that is the whole point of having written it.

## 4. Cancellation still restores everything

- [x] 4.1 Test that cancelling an ordered destruction within the window brings
      the mentions and the items back, matching what the capture says. This is
      what the fold's keeping of the text is for, and a withholding that could
      not be undone would have taken the cancellation window with it.

## 5. Rewrite the two tests that assert the defect

- [x] 5.1 `tests/entities.rs` — the case asserting a queued note is still on the
      entity page. Rewrite to assert the rule and correct the comment in place;
      it records that somebody believed this was correct, which is worth keeping.
- [x] 5.2 `tests/checklist.rs` — the case asserting two items are readable while
      queued, whose comment claims "nothing leaks". Same treatment.

## 6. Guards and verification

- [x] 6.1 Confirm no `PROJECTION_SCHEMA` bump is needed, by test rather than by
      argument: order a destruction and assert the note's derived rows are gone
      without any rebuild, which is what proves the incremental merge re-writes
      an affected note at the moment the order is given. Covered by
      `a_destroyed_capture_has_no_items`, whose queued phase reads through an
      ordinary `Session::open` with no rebuild and finds the rows already gone.
- [x] 6.2 Run `make duplicated` — the accessor must not have become a second
      copy of a condition that already existed.
- [x] 6.3 Run `make ci`. Same as above: green except `deny`, which is blocked
      from reaching github.com here. `make test` exits 0, 73 suites, and the
      exit code was read from `make` itself rather than from an `echo` after it.
- [x] 6.4 Check whether any *other* table in `write_into` is filled from the
      text under a guard of its own. The rule has now been got wrong three times
      in this one function, so the question is worth asking exhaustively rather
      than table by table as each leak is found. **Done before implementing,
      and it found `links`** — see 2.4. `tags`, `tag_adds`, `ambient`,
      `parents`, `divergences`, `note_versions` and `held` are built from events
      rather than from the text, and correctly survive.
