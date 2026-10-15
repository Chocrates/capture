## D1 — A warning is a moment of its own

A warning is a `Due` at `start - lead` carrying `ahead = lead`. Delivery is
keyed by `(note, occurrence, instant)`, so it already has its own record and
nothing in the delivery path changes: it is said once, a correction to the time
moves it, and quiet hours and staleness treat it like any timed moment.

It is produced by one function (`Store::announce_by`) that every caller of
`due_by` passes its result through, rather than inside `due_by`, so the
projection keeps answering "what was named for when" and the policy about
announcing it lives with the rest of the notification policy.

A warning whose moment has already begun is dropped. A capture made ten
minutes before a meeting otherwise said "in 15 min" for something already
underway, and then said it again at the start.

## D2 — The lead is the speaking device's setting

Only one device speaks (§2.4.1), so the lead is local operational state on that
device (`alerts.lead_min` in ops), not a logged fact. Default 15. Zero turns
warnings off. The desktop overlay reads the same key from its own ops store.

## D3 — Loud means the alarm stream, repeated

Android's per-channel sound cannot be changed after the channel exists, so
timed alerts move to a new channel `alarms`: alarm ringtone, `USAGE_ALARM`
audio attributes (it follows the alarm volume, not the notification volume),
vibration on, and `FLAG_INSISTENT` so the sound repeats until the notification
is dismissed or opened. Day-only alerts stay on `alerts`, because a task due
today announcing itself like an alarm three times a day is the manufactured
pressure §3.6 rules out.

Not done: bypassing Do Not Disturb (needs policy access granted by hand) and a
full-screen lock-screen alarm. Both are possible later if this is still not
enough.
