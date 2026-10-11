## 1. Tests first

- [x] 1.1 Add `serde_json` to `capture-cli`'s dev-dependencies.
- [x] 1.2 Write `apps/capture-cli/tests/json.rs`: one store holding a hostile
      note (quotes, backslash, newline, a control byte), a task, an event, a
      list, a link, a mute; run every `--json` command and parse each output.
- [x] 1.3 Failing tests for each drift in the proposal: agenda overdue/owed and
      `cancelled`; due state and unread; show promotion and links; lapsed mute
      reports `muted: false` with `muted_until`; tasks text; review suggestion;
      peers sync; export withheld; quarantine/held reason escaped.

## 2. The mechanism

- [x] 2.1 `json.rs`: `Json`, `obj`, `render`, `quote` (moved), with unit tests.
- [x] 2.2 Rewrite every read command in `read.rs` as build → view → two
      renderers, prose taking only the view.
- [x] 2.3 The same for `ops.rs` (`log`, `peers`, `quarantine`, `held`,
      `rejected`), `verify.rs`, `backup.rs` (`export`), `write.rs` (`settle`,
      `recover`).
- [x] 2.4 Confirm prose is unchanged: the existing suites pass without edits to
      prose assertions, and the pre-change binary and this one print
      byte-identical prose for 28 commands over one store (export differs only
      in the store's path).
- [x] 2.5 Add `scenarios` and `json` to `make ci`.

## 3. Close-out

- [x] 3.1 Sabotage each new test and confirm it fails.
- [x] 3.2 `make ci` (`deny` excepted — no network here).
- [x] 3.3 `tools/unwired.py`, `tools/unreached.py`, `tools/duplicated.py` quiet.
- [x] 3.4 Ledger entry in `~/.local/share/capture-reviews/PROGRESS.md`.
- [x] 3.5 Archive.
