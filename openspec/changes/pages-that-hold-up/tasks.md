## 1. Defects first (tests that fail today, then the fix)

- [x] 1.1 G17 test, then D1: a flagged title writes no alias; `Created` carries the reason; CLI/FFI/desktop print it.
- [x] 1.2 G13 test (**property**: arbitrary strings never error), then D2 quoting in `Store::search`; last term keeps prefix match.
- [x] 1.3 G14 test, then D2 ranking: title as its own FTS column (new table, projection version bump), `bm25` weights.
- [x] 1.4 G17 edit half, then D3: fold flags a note when any version is flagged; surface names the version; re-fold reports how many flags changed.
- [x] 1.5 G18 test (**property**: interleaved writers with stale/fresh expects never lose a version), then D4: `mutate::edit(.., expect)`, `ExitCode::Conflict`, check inside the writer lock; `show --json` exposes the version id.

  - 2026-11-13: 1.1–1.4 built, each sabotaged once and seen to fail. One wording for every surface (`Created::notice`); an emoji-only title now gets no notice (it used to say "taken", which was wrong). Search index is `note_words(title, body)`, projection schema 17. Not done from 1.4: naming *which* version tripped the flag, and a count of flags that changed on re-fold — the schema bump rebuilds anyway; 6.2 checks the result by eye instead.
  - 1.5: `mutate::edit_expecting`, `Error::Conflict` → exit 4; `show --json` gives `version` (full id). The property test needed 32 cases to catch its sabotage reliably (12 missed it once). The CLI flag itself is `an-agent-keeps-notes-through-the-cli` 4.2.

## 2. Gaps at the store layer

- [x] 2.1 G6–G8: rename, reassigned name, forward link.
- [x] 2.2 G9 (**property** over `name_for`): accents, CJK, RTL, emoji-only → `None`, mixed scripts; no panic, stable output.
  - 2.2 found a real bug: `name_for` split on every non-alphanumeric *character*, so "हिन्दी नोट्स" → `हिन-दी-नोट-स`, "İstanbul" → `i-stanbul`, a decomposed "café" → a different name from the precomposed one. Now NFC + grapheme clusters (word boundaries were tried: they split CJK per ideograph). Native script kept, not transliterated. Left alone: a `[[link]]` typed in decomposed form still won't match an NFC name — names compare as exact strings.
- [x] 2.3 G10: redaction of a page with backlinks and children (store half).
  - Found a second real bug: a destroyed page kept its alias. The alias is the title's words, `[[trip]]` went on resolving to the tombstone (so the link never showed broken), and no new page could take the name. A rebuild already dropped it (the alias body is destroyed with the rest); the in-place purge did not. Purge now clears it. Children of a destroyed page show no parent (filtered where the tree is read; their own parent events are theirs and stay).

## 3. Gaps across two devices (`tests/two_devices.rs`)

- [x] 3.1 G1 (**property**: random interleavings of edits and syncs on two devices converge to the same tip and divergence count).
- [x] 3.2 G2 resolve crosses sync.
- [x] 3.3 G3 (**property**: tick vs reword/insert/remove races never tick a different item).
- [x] 3.4 G4, G5: `Page` promote crosses sync; make-page races an edit.

  - 3.3 found a third real bug, in `reconcile_ticks` rule 3 (the position fallback): remove "eggs" and add "butter" in one edit and the count is unchanged, so "milk" inherited the tick. One device ticking while the other edits hits it. Rule 3 now also needs the row to still read like the item (a shared word, or a typo apart — `similar`). Cost: a reword to wholly different words ("milk" → "oat drink") drops the tick instead of keeping it; the tick stays in the log. Single-device regression in `checklist.rs`; G3 is exhaustive over six edit kinds × tick-before/after rather than sampled. Sabotaged: G3 fails on edit 4 without the guard.

## 4. Surfaces

- [x] 4.1 G10 CLI half: confirmation names linkers and children.
- [x] 4.2 G11: `capture log <page>` lists every version with origin (scenario test).
- [x] 4.3 G15: decide and test `--under` with `--of`.
- [x] 4.4 G16: phone instrumented test — new page, edit, tick, link, unpage.

  - 4.1 also names, without `--with-links`, the notes whose links will break. 4.3: refused, either order, "not both" (it was already refused by accident, with a message about argument count). 4.4 ran on the API 35 emulator: 8 tests, 0 failures, `ConvergeTest` skipped as it always is without a desktop half. The emulator needed `-wipe-data` first: the debug APK is 287 MB (two unstripped 130 MB `.so`) and would not fit; no third-party apps were on it.

## 5. Size

- [x] 5.1 G12: bench a 1 MB page and a 10 k-line checklist (rebuild, search, `show`); record numbers in this file; set a regression budget in `make ci` at 2× the measured value.

  - Measured 2026-11-13, debug build, three runs, stable to ±3%: edit a 1 MB page 2367 ms; edit a 10k-line checklist 808 ms; tick one of those lines 854 ms; rebuild 2968 ms; search 1 ms; read 10k lines 18 ms. Budgets ≈2× in `tests/large_pages.rs`; `make size` is in `make ci`; `CAPTURE_SIZE_BUDGET_SCALE` for slower machines. The tick costs as much as the edit — every write re-folds the note and re-expands all 10k items. Fine at this size; worth knowing if an agent grows a list.

## 6. Verification

- [x] 6.1 `make ci` and `make android` green; sabotage each new test once (break the fix, see it fail).
  - 2026-11-13: every suite in `make ci` passed (test, pty, tamper, roundtrip, spool, redact, scenarios, json, roster, size, purity, unwired, unreached, duplicated, aside). `deny` failed on advisories new in today's database and unrelated to this change: `libsqlite3-sys` (bundled C SQLite CVE-2026-119375), `basic-toml` unmaintained, `xattr` unsound, `clipboard-win` yanked — its own follow-up. Android: app built, instrumented suite green on the emulator, `android-decides` passes; `android-test` (the store suite cross-run on a device) not run. Every fix sabotaged once and its test seen to fail: 1.1, 1.2, 1.3, 1.4, 1.5, 2.2, 2.3 (alias and parent filter), 3.3, 4.1.
- [ ] 6.2 Deploy; re-fold on the real archive; read the flag-change count and check the newly flagged notes by eye.
