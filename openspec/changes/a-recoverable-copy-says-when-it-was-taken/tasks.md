# Tasks

## 1. The line

- [x] 1.1 Extract the prose line into a pure function taking the snapshot, the
      clock and the zone.
- [x] 1.2 It says when the copy was taken, as a date in the reading zone.
- [x] 1.3 A unit test, which is the first in this file — assert the date is
      present and that the first line still is.

## 2. The machine-readable form

- [x] 2.1 `--json` carries `taken_at` in milliseconds, beside `expires_in_ms`.
- [x] 2.2 Check nothing in the repo parses the old shape. Adding a field is not
      a break, but the check is cheap and the last listing I changed had no
      consumer at all. Nothing reads `expires_in_ms` either — same answer.

## 3. Guards

- [x] 3.1 Verify the unit test is non-vacuous by dropping the date again.
- [x] 3.2 `make ci`. 78 suites green; `deny` fails on network only. Clippy caught
      a redundant `.clone()` — `Zone` is `Copy`.

## 4. The gap this leaves, stated

- [x] 4.1 Record that nothing tests `capture recover` end to end, and why: a
      snapshot needs a carried-out redaction, the window is thirty minutes of
      real time, and the CLI has no clock to inject. The pure function covers
      the rendering and not the wiring. Recorded in the review ledger as a
      standing gap, with the clock injection named as what would close it.
