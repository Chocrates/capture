## Context

§2.4.1: "Until you choose, the notifier follows a compiled-in default rule — the
most recently paired device that can notify — evaluated locally and never
written to the log." And: "Fallback is an ordered list, not a single primary …
If the primary has not been seen by a device for longer than a grace window,
the next in the list."

## Decisions

### Order: first admission, by HLC

The roster is already built in HLC order, so recording the HLC at the moment a
key is first inserted costs one map. Re-admission after a retirement does not
reset it: the device was paired when it was paired.

Every device folds the same membership events, so every device computes the
same order. That is what lets a locally evaluated rule agree without anything
being written.

### "Can notify" is negative: `silent`

`observes` is a list of strings exchanged in the anti-entropy hello and stored
per peer, local-only. A device adds `silent` when its own notifications are
blocked (the phone's `alerts.silenced`). A positive `notify` would be absent from
every older build's list, and the rule would skip every peer that had not
upgraded — the safe-looking choice that produces silence.

### Liveness: reached within 24 hours

From `peer_addresses().last_seen`, which a round already records in both
directions. Never reached counts as not live, so a phone paired a minute ago
does not take over until a round has actually completed with it — before that,
the device that can see the moment keeps announcing it.

Twenty-four hours, not minutes: a phone in a drawer for an afternoon should not
hand alerts back to the desktop and then take them again; a phone that has
not been seen for a day probably is not the place somebody will see a toast.

### Self is a candidate like any other

This device skips itself if it is silent, and otherwise is placed by its own
admission time. It never applies the liveness check to itself.

### Where the inputs come from

`speaks_here(paths, state, me, now)`: the roster from the log (as `capture
peers` does), capabilities and last-seen from ops, this device's own silence
from ops. The pure part, `default_notifier`, takes all of that as arguments and
is what is tested.

## Risks

- Two devices whose views of liveness differ may both speak, for up to a day in
  the worst case. Accepted: duplicate is the cost §2.4.1 tolerates, silence is
  the failure it names.
- A desktop with no notification daemon would not report `silent`. It has no
  way to know; that is a pre-existing limit of the desktop path.
