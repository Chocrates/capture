## Why

With nobody having chosen a notifier, every paired device announces every
time cue. `notifier::speaks_here` returns true when no `setting` exists, and
`default_rule()` still describes the M1 situation — "this device — the only one
paired" — on an archive with three devices in its roster.

DESIGN §2.4.1 states the default: **"the most recently paired device that can
notify"**, with "an ordered list, not a single primary" so that a dead or silent
primary does not mean silence. None of that was built. The phone and desktop
were paired on 2026-10-10 and from then on each would have announced every
moment the other announced too — the duplicate §2.4.1 opens by naming as the
only goal ("the only goal is avoiding duplicate work").

## What Changes

- **The roster remembers when each key first joined.** `Roster::admitted`, the
  HLC of the first event that made a key a member — the root, a `device_add`, a
  re-admission. *First*, because pairing writes two admissions (each side admits
  the other) and the second would otherwise make the older device look newer.
- **A device says when it cannot speak.** The capability list exchanged every
  round (`observes`) gains `silent` when this device's notifications are
  blocked. A negative token rather than a positive `notify`: older builds report
  no such token, and absence must read as "can", or every older peer would be
  skipped.
- **`notifier::default_notifier`, a pure rule** every device evaluates the same
  way: roster members, most recently admitted first; skip any that reported
  `silent`; skip any peer this device has not reached within a grace window
  (24 h) or ever; the first left speaks; if none, this device speaks.
- `speaks_here` applies it when no setting exists, and `capture status` and the
  phone name the device the rule currently picks instead of "the only one
  paired".

## Capabilities

### Modified Capabilities

- `intents`: the default rule is specified rather than left to "a compiled-in
  rule", because "every device" is a rule too and it was the wrong one.

## Impact

- `capture-core` roster (new field), `capture-store` notifier/sync/session,
  `capture-ffi`, `capture-overlay` intents, `capture-cli` status.
- The phone becomes the default speaker when paired after the desktop, which is
  what §2.4.1 expects ("In practice this is the phone").
- A device may still announce a moment its peer also announces during a window
  where their views of each other differ (one has reached the other more
  recently). Duplicates in that window are accepted; silence is not — the rule
  falls back to self when it can find nobody.
