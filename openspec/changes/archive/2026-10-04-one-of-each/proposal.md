## Why

Several small facts about this system are written down more than once, and the
copies have already begun to disagree.

Counted rather than estimated:

| what | copies | where |
|---|---|---|
| Reading the local timezone from `$TZ` / `/etc/localtime` | **4** | `notifier.rs`, cli `read.rs`, cli `write.rs`, overlay `intents.rs` |
| Rendering a device key as six hex bytes | **7** | `round.rs`, cli `ops.rs`, cli `pair.rs`, desktop `ipc.rs`, ffi ×3 |
| `hexed` / `unhex` | **2 each** | cli `pair.rs`, ffi |
| A test's `Scratch` store fixture | **9** | every suite in `capture-store/tests` |
| "Open the current generation read-only" | **~20 call sites** | everywhere |

None of this is abstraction for its own sake. Each one is a decision the
system has already made once, and a copy is a place it can be made
differently:

- **The zone.** A wrong answer moves quiet hours by the UTC offset — the
  difference between a notification at 07:00 and one at 02:00. Four copies is
  four chances to fix the `/etc/localtime` parse in three places.
- **The short key.** It is what a person compares when deciding whether to
  retire a device. Seven copies of "six bytes" is a number that can become
  five in one surface, so two screens name the same device differently at the
  moment somebody is trying to tell devices apart.
- **`unhex`.** The two copies already differ: one rejects an empty string and
  the other accepts it, returning an empty vector. They are used either side
  of the two-signature readmission of §5.3.

This is also a report on how it got here, because the pattern matters more
than the count: **most of these were added in the last week, and several by
following a local example rather than looking for an owner.** The overlay's
`local_zone` was copied from the command line's because the command line's was
right there. That is the mechanism, and it will keep producing copies until
the things have an obvious home.

## What Changes

- **One `local_zone`**, in `capture-store`'s `notifier` beside `deliver`, which
  is the only rule that needs it. The other three call it.
- **One short-key rendering**, in `capture-core` next to the key type it
  renders.
- **One `hexed` / `unhex` pair**, likewise, with the empty-string question
  answered once.
- **One `Scratch` fixture** for `capture-store`'s suites, in a shared test
  module.
- **One way to open the current generation read-only**, which is the most
  repeated expression in the codebase.

**No behaviour changes.** Where two copies differ today, the difference is
resolved in favour of the stricter one and that choice is written down.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. Nothing here changes what the system does; it changes how many places
decide it.

## Impact

`capture-core` (key rendering, hex), `capture-store` (`local_zone`, a
read-only-open helper, a test fixture), `capture-ffi`, `apps/capture-cli`,
`apps/capture-desktop`, `apps/capture-overlay`.

**A guard, so this stops recurring.** `make unwired` already refuses a public
item with no caller. The same shape can refuse a *second* definition of a name
that already exists elsewhere in the workspace — which would have caught
`unhexed` beside `unhex`, and `local_zone` the third time.

## Non-goals

- **Deduplicating prose.** The long comments explaining *why* a thing is done
  are not duplication; several deliberately repeat an argument next to the code
  it constrains, because the alternative is a reader following a reference to
  find out why the line in front of them exists.
- **Merging the four `capture_body_*` constructors.** They differ in what a
  capture carries — a source URL, a blob, a zone — and collapsing them into one
  function with four optional arguments is a worse shape, not a better one.
- **Unifying `summarise` / `rendered` across the FFI and the desktop.** They
  produce different records for different surfaces. What they share is already
  shared: the projection answers the questions, and both read it.
- **A general-purpose utility crate.** Each thing moves to where it belongs —
  key rendering beside the key, the zone beside the rule that uses it — and not
  to a drawer named for having no home.
