# the-window-agenda-uses-the-day-you-are-in

## Why

The window's agenda asks the store for everything happening between the start
and the end of a day. It builds that window in **UTC**:

```rust
let zone = capture_time::Zone::new("UTC").expect("UTC is always present");
let from = capture_time::at_local(date, 0, 0, zone)?;
let to = from + 24 * 60 * 60 * 1000;
```

So "the agenda for 2026-10-05" means 00:00–24:00 UTC, not the day the reader is
living in. For anyone west of UTC the window is shifted late; for anyone east it
is shifted early.

Demonstrated, not inferred. On a scratch store in `America/Denver`:

```
capture + "dinner 2026-10-05 19:00"
  stored instant   1791248400000  =  2026-10-06 01:00 UTC
  capture agenda 2026-10-05       →  19:00  dinner        (correct)
  window the window uses          →  1791158400000 .. 1791244800000
  the dinner is 1h past the end of it
```

So the window shows an empty evening on the 5th and puts the dinner on the 6th,
while the terminal shows it on the 5th. Two surfaces, one store, different
answers about what day something is on — and the one that is wrong is the one
somebody is most likely to be reading at 19:00.

Every evening appointment is affected, which is most of them.

The same mistake sets the overdue comparison a few lines down —
`civil_date(now_ms(), zone)` with that same UTC zone decides whether the day
being viewed is "today", and therefore whether overdue tasks are shown at all.
The spec defines the overdue boundary against the reading zone.

The CLI has always used the local zone here. `ipc::tasks`, added earlier today,
takes the system zone deliberately. The agenda is the one left.

## What Changes

- The window's agenda builds its day from the system zone, the way the terminal
  does, so a day means the day the reader is in.
- The overdue comparison uses the same zone, so "is this today" is asked in the
  zone the boundary is defined against.
- The zone is resolved once and used for both, rather than twice with a chance
  to differ.

## Capabilities

### Modified Capabilities

- `browse` — a day on the window's agenda is the reader's day.

## Non-goals

- **No change to all-day entries.** They are floating dates with no position on
  an hour axis (§4.4) and are matched by date string, not by instant, so they
  were never affected.
- **No change to what is stored.** Instants are instants. This is about which
  window of them a day means.
- **No zone picker.** The system zone is what the terminal uses and what the
  phone uses; a third answer about which zone the reader is in is not wanted.
