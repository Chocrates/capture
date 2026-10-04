# a-recoverable-copy-says-when-it-was-taken

## Why

"WHEN a redaction has been carried out and its snapshot has not expired, THEN it
is listed, with **when it was taken** and what it begins with."

It is listed with its id, how long until it expires, and its first line. When it
was taken appears nowhere — not in the prose listing, not in `--json`.

`Snapshot.taken_at` is stored, selected, and `ORDER BY`-ed. Its only reader in
the entire tree is one assertion in a store test. So the field exists, is
carried all the way to the surface, and is dropped there.

The omission matters more than a missing column usually would. These are
plaintext copies of content somebody ordered destroyed, kept for thirty days.
Deciding whether a copy is the one you want means knowing *which* destruction it
belongs to, and "expires in 27d" answers that only by arithmetic the reader has
to do. The date you asked for something to be destroyed is the thing you
actually remember.

## What Changes

- The prose listing says when each copy was taken, as a date in the reading
  zone, alongside what it begins with.
- `--json` carries `taken_at` in milliseconds, which is what a consumer can do
  something with, beside the `expires_in_ms` it already has.
- The line is rendered by a pure function with a unit test, because nothing else
  can reach it: producing a snapshot needs a redaction that has actually been
  carried out, and the cancellation window is thirty minutes of real time, so no
  CLI test can get there without a clock to inject. That is also why this went
  unnoticed — `capture recover` is invoked by no test at all.

## Capabilities

### Modified Capabilities

- `cli` — a recoverable copy says when it was taken.

## Non-goals

- **No change to what is kept or for how long.** Thirty days, §1.2, untouched.
- **No second place the destroyed text is printed.** The listing shows the first
  line only, deliberately, and that stays — the point is to recognise a copy,
  not to read it back out of a listing.
- **No clock injection for the CLI.** It would make a test possible here and is
  a much larger decision about the whole binary; the pure function gets the same
  coverage for this change without it.
