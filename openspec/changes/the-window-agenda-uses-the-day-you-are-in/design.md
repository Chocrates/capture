# Design

## What the UTC zone was doing there

Two uses, one line apart, and they need separating to see which is wrong.

```rust
let zone = capture_time::Zone::new("UTC").expect("UTC is always present");
let from = capture_time::at_local(date, 0, 0, zone)?;   // the day's window
...
let today = format!("{}", capture_time::civil_date(now_ms() as i64, zone));
```

The first turns a floating date into an instant. That is exactly the operation
§3.6 warns about — "due Friday is not 00:00 UTC on Friday" — and doing it in UTC
makes the agenda's day a 24-hour span that only lines up with somebody's day if
they happen to live on the meridian.

The second asks what today is, to decide whether to show overdue work. The
overdue boundary is defined against the reading zone, so asking in UTC gets a
different answer for several hours either side of midnight.

Both want the system zone. There is no reading of this where UTC is right, which
is worth saying because `Zone::new("UTC").expect(..)` reads like a deliberate
choice — it is infallible and the `expect` makes it look considered. It is more
likely the convenient zone to construct when the surrounding code needed *a*
zone.

## Resolved once

The terminal does this:

```rust
let zone = capture_time::Zone::new(&local_zone()).unwrap_or_else(utc);
```

UTC as the fallback, not the answer: if the system cannot say where it is, a day
still has to mean something. The window gets the same, resolved once at the top
and used for both the window and the comparison — two resolutions could differ
if the zone changed between them, which is a small risk and a free one to
remove.

## All-day entries were never affected

They are matched by date string, through `all_day_on(&day)`, because a floating
date has no position on an hour axis (§4.4). So they land on the right day under
either zone, and they are left alone.

That is also why this went unnoticed for so long: the entries most likely to be
glanced at in a test — a dentist appointment on a date, an all-day range — are
exactly the ones the bug does not touch.

## Testing it

The window's agenda is not reachable from a test: `capture-desktop` is a binary
crate, and `frontend.rs` reads the page as text.

So this is verified the way the defect was found — against the terminal, which
reads the same store with the right zone. An evening appointment appears on one
day there, and the arithmetic of the window's own bounds shows it fell outside.
Written into the change rather than left in a shell session.

The honest limit: nothing will catch this returning. Making `ipc` testable means
a library target for the desktop crate, which is a bigger change than this one
and worth considering on its own — the window now has four commands that were
written this session, and none of them can be tested below the page.
