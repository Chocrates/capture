## 1. The encoding, frozen before anything writes it

- [x] 1.1 Kinds 30–32 in `event.rs`: `ENTITY_RENAME`, `ENTITY_MERGE`, `ENTITY_BIND`. Widen `is_known()` to `1..=32` and update both permanence tests — `kind_numbers_are_permanent` and `the_new_kinds_are_known` — plus the `!Kind(30).is_known()` assertion that currently passes by accident of the boundary.
- [x] 1.2 Bodies in `body.rs`, each opening with a 16-byte nonce: `EntityRenameBody { ent_id, display }`, `EntityMergeBody { from, into }`, `EntityBindBody { ent_id, note_tgt: Option<EventId> }`. `Option` for the same reason `ParentBody` has one — unbinding must be able to beat an earlier bind.
- [x] 1.3 `MuteScope::Entity(ent_id)` as `#[n(3)]`. An added variant, so the three that exist keep their numbers and their encodings. **The fold cannot resolve it**: which notes name an entity is a question about their text, and `capture_parse` already depends on `capture-core`. So `apply_mutes` carries the rule out whole into `Projection::entity_mutes`, and `Note` gains `muted_at` (`#[n(26)]`) — without a timestamp, a mute resolved later in the store would beat every note-scoped mute by being applied later rather than by being newer.
- [x] 1.4 Golden vectors for all three kinds and for the new mute scope, **before any writer exists**. `event-log/spec.md:9` requires it and the encoding is frozen from first write: a mistake here is permanent.

## 2. Identity in the fold

- [x] 2.1 **Simpler than specified, and the spec overstated it.** Because `casefold(name)` *is* the hashed key, every spelling folding to the same string already yields the same `ent_id` — so first-sighting never enters identity and `ent_id` is permutation-invariant by construction. First sighting decides the *display* name only, and that is computed in the store, which is where mentions are. The property test therefore belongs to display, not identity.
- [x] 2.2 `entity_display` `#[n(5)]`, `entity_merged` `#[n(6)]`, `entity_backing` `#[n(7)]` on `Projection`, all inside `canonical()`. Plus `entity_of`, `entity_name`, `entity_backing_of` as the resolvers the store will read through.
- [x] 2.3 `apply_entity_rename`, `apply_entity_merge`, `apply_entity_bind` on the `apply_parent` shape. An empty display name is `Held::Malformed` rather than stored; a bind naming an absent note is `Held::NoTarget`; merging an entity into itself is dropped.
- [x] 2.4 **Not needed, and the spec was reasoning from the wrong analogy.** `apply_alias` runs inside the main pass because a *link* written in the same walk resolves against the names then current. An entity event is keyed by `ent_id`, which no event can change, so there is no ordering hazard to avoid — the three arms sit in the main pass with the rest and nothing depends on where.
- [x] 2.5 `break_merge_cycles`, mirroring `break_parent_cycles`: find the loop, drop its newest edge by `(at, id)`, identically on every device. `entity_of` also bounds its own walk by the edge count, so a projection assembled some other way still terminates.
- [x] 2.6 Ten tests: chain of three resolves to its end, idempotent, order-independent, self-merge is a no-op, a cycle leaves both reachable and both devices agree, unbinding beats an earlier bind, and a renamed entity answers through its merge.

## 3. The projection

- [x] 3.1 Both tables added. `entity_names` stores the **resolved** id, not the derived one — `entities` only holds the survivor of a merge chain, so a lookup landing on a merged-away id would find no row at all.
- [x] 3.2 `PROJECTION_SCHEMA` 7 → 8 with its changelog entry. All 269 existing store tests pass under it.
- [x] 3.3 `write_entities` runs last in `write_into`, recomputing **whole** rather than per note: an entity's display is the earliest spelling written for it *anywhere*, so one arriving note can change what it is called. Ordered by `(created, tgt)` so two devices pick the same spelling. `mentions` keeps its shape and index. Entity mutes land via `muted_for`, which is the hand-off `muted_at` exists for.
- [x] 3.4 `entity_id`, `entity`, `entity_by_id`, `entity_notes`. The join folds `mentions.value` with `lower()` to meet `entity_names.name`, which is stored folded — they have to agree exactly or a name reaches no entity. Backing note excluded from the list it sits above.
- [x] 3.5 `entities_of_kind(kind, now)` with counts, plus `EntityRow` so no surface assembles a page from parts. Merged-away entities are absent without anything removing them, because only survivors get rows.
- [x] 3.6 **The test failed and found a real pre-existing bug.** `purge_note` deleted `note_fts`, `fts_map` and `links` but never `mentions` — so a *destroyed* note went on naming every person, project and tag it had mentioned, on every one of their pages, for ever. No text was readable, which is why nothing looked wrong; what survived was the fact of the mention, which is the part §1.1 is about. `list_items` had the same hole. Both now deleted. The test also pins the queued-but-not-destroyed window, where withholding is a read-time `CASE` and the mention row legitimately remains.

## 4. Incremental fold

- [x] 4.1 All three kinds global. Not an optimisation: a partial fold does not hold the note an `entity_bind` names, so the bind was held as `NoTarget` and silently dropped. Caught by the backing-note test failing before this landed.
- [x] 4.2 Two cross-device tests in `two_devices.rs`, over a real socket with real identities: a rename crosses and does not fork the page, and a merge crosses so either spelling reaches the survivor on both devices.
- [x] 4.3 Three spellings of one name (`@Sarah`, `@sarah`, `@SARAH`) give one person with all three notes, and the display is the earliest capture's spelling. Identity is order-proof by construction — `casefold(name)` is the hashed key — so what the test actually pins is the display rule, which is the only order-sensitive part.

## 5. Writers and the command line

- [x] 5.1 `rename_entity`, `merge_entities`, `bind_backing`, `mute_entity` in `mutate.rs`, each resolving spellings to ids first so an event never carries a word. `resolve_entity` refuses with prose where no note has ever used the spelling. `mute_scope` already took any scope, so entity muting needed no new writer.
- [x] 5.2 **The rationale was wrong and the fix was still needed.** Entity events carry no `tgt`, so they never reach `capture log`, which is note-scoped — they would not have printed "unknown" there. But fifteen *targeted* kinds already did: `promote`, `intent`, `review`, every `occ_*`, `parent`, both `item_*` and the roster kinds, so `capture log` blamed a newer client for events this build wrote itself. All named now, pinned by a test over `1..=32` so the next kind added fails there instead of joining them. Worth noting separately: entity events are visible in no log surface at all, because there is no global event view.
- [x] 5.3 `capture people` and `capture projects`, sorted by how much is said about each, showing extra spellings after a merge. Empty states say the axis populates itself rather than reading as unconfigured.
- [x] 5.4 `capture entity @sarah` with names, backing note and timeline, plus `entity-rename`, `entity-merge`, `entity-bind` (no note unbinds). Help text extended; parse tests for arity and for the absent-argument unbind. Exercised end to end against a scratch store: `@Sarah`/`@sarah` come back as one person with both notes, a merge gives one page reachable by either spelling, and a bound backing note prints above the list and is absent from it.
- [x] 5.5 `mute_entity` wraps `mute_scope`; tested holding and lapsing without anything being restored.

## 6. The binding

- [x] 6.1 `people()`, `projects()`, `entity(mention)`, `renameEntity`, `mergeEntities`, `bindBacking`. No generic `mentioning`. `entity` takes a mention as written (`@sarah`) and `split_mention` reads the sigil **in Rust**, so what `@` means is decided once, in the place that owns it.
- [x] 6.2 `Entity { display, kind, names, backing, notes }`. `count` dropped as redundant — `notes.len()` is the count and carrying both invites them to disagree. `names` is what a page needs instead: the spellings that also reach it, which is how it says what it has been called.
- [x] 6.3 Five tests through the binding: case does not fork a person, a rename keeps both notes, a merge is followed from either spelling and is refused across kinds, a bind leaves a task a task and still outstanding, and a mention that is not one is refused rather than guessed at. 47 pass.
- [x] 6.4 Regenerated; `Entity` and all six methods present, Android compiles clean before any screen exists.

## 7. The phone

- [x] 7.1 People and projects as banded sections on the Pages tab, with note counts and the extra spellings a merge leaves. They come first, because they populate without anybody doing anything and so are the sections likely to hold something.
- [x] 7.2 `EntityPage`: display name, kind and count in the header, extra spellings under it, backing note above a divider, notes below. The page arrives whole from the archive — nothing here assembles it from a query and a count.
- [x] 7.3 `MentionedText` renders the note body as one `AnnotatedString` with the mentions underlined and tappable. The runs come from a new FFI `pieces(text)` that mirrors the desktop's own splitter, so what counts as a mention is decided once in Rust — `make android-decides` still reports "22 Kotlin file(s), none of them deciding anything". One `AnnotatedString` rather than a row of `Text`s, or the paragraph would break lines at the mentions instead of at the words.
- [x] 7.4 Rename, merge-into and bind/unbind on the page, each behind a dialog that says what it will do. Merge is phrased `from → this page`, so the surface somebody is looking at is the one that survives, and the dialog says nothing moves.
- [x] 7.5 Empty index distinguishes the two cases: a page is a note you named, while people and projects are not made at all and appear when a capture mentions one. An absent entity page says the same rather than reading as a missing record.

## 8. The desktop

- [x] 8.1 `mentioning` goes through the entity, so a page holds what *either* spelling named after a merge and is titled by a chosen name after a rename. `wiki_index` lists people and projects as entities, so a merged-away spelling leaves the sidebar instead of sitting there with an empty page. **Tags deliberately keep the old path**: `distinct_mentions('#')` unions text mentions with the OR-Set a `tag` event maintains, and an entity is derived from text alone — swapping it would have silently dropped every hand-added tag from the sidebar. Caught before it shipped; it is also the non-goal this change declared.
- [x] 8.2 `rename_entity`, `merge_entities`, `bind_backing` over IPC, and three buttons on the page they act on. A merge lands on the survivor rather than the page that just stopped existing. Not offered for tags, which have no identity of their own here.

## 9. Seeing it

- [~] 9.1 **Partly done, and it earned its keep immediately.** Driven on a Pixel 8 Pro against `dev.capture.dev`, screenshotted at each step: the Pages tab shows People and Projects with counts, `@sarah` and `@Sarah` arrive as one person holding both notes, and the entity page renders its header, actions and timeline with the Pages tab correctly still selected. Two real bugs fell out of it — see below. **Outstanding**: the merge through the UI (the phone locked mid-run), two people sharing a display name, a merge chain of three, and a redacted note that used to name somebody.
- [~] 9.2 **The property is proved; the on-device leg is not.** `two_devices.rs` renames on one device and merges on another over a real socket with real identities, and asserts the other agrees, that the page does not fork, and that either spelling reaches the survivor. Driving the same path from the phone to a live `capture` daemon is still outstanding — the phone left the network before it could be finished. **Emulator or `dev.capture.dev` only** — never the real archive.
- [x] 9.3 **Measured, and it was slow: 486ms** for the people index at 301 people — worse than the 66ms `read_as` case, on a surface somebody opens to browse. `entities_of_kind` was asking `entity_notes` once per row, each dragging its own `entity_by_id` and `spellings_of`. Replaced with three grouped queries: **6.65ms**, 73× faster, identical answers. The widest page (60 notes) was already fine at 2.4ms. Kept as an ignored test rather than a CI threshold.
- [x] 9.4 A store carrying a rename and a merge is rebuilt into a fresh generation and every entity, display name, spelling set, backing note and count comes back identical.

## 10. Found by driving a real device

- [x] 10.1 **Every error this application has ever shown began with `reason=`.** uniffi generates `override val message get() = "reason=${reason}"` for an error variant, and `Model.load` passed it straight to the screen — a Rust field name, in front of a sentence written for a person, app-wide since the binding existed. Stripped in one place; the wording still comes from Rust.
- [x] 10.2 **Merging demanded the sigil twice.** On `@sarah`'s page the dialog asks for the other name, and typing `sarah-chen` — the obvious thing, since the screen already says it is a person — was refused. `merge_entities` now accepts the other name bare and takes the kind from the page. The entity being merged *into* still needs its sigil, because nothing else there says what kind it is.
