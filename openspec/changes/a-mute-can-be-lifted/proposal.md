## Why

A mute cannot be lifted. `capture mute <note>` suppresses a note from search,
from review, and from every entity page naming it, permanently, and no surface
on any device can undo it.

This is not a missing nicety. It contradicts a requirement that is already
written down twice:

- `openspec/specs/review/spec.md` — "the note is not destroyed and **the mute is
  reversible**".
- DESIGN.md §2.6 — a mute suppresses "permanently **or until a stated date**",
  and "**It is reversible from any device, at any time**."

§2.6 also gives the reason the gap matters rather than merely being untidy:
muting exists so that making something go away is *cheaper* than destroying it.
"A system that pushes personal content on a schedule the user is explicitly
barred from setting must have a suppress that is not destruction." An
irreversible suppress is not that. It makes `mute` the second one-way operation
in a system whose §6 reserves one-way-ness for redaction — the single operation
that is irreversible because it destroys something. A mute destroys nothing, and
`write.rs`'s own module header claims "there is no operation in this file that
can lose something".

The reason to do it now is that it is nearly free. The mechanism is already
built, folded, and threaded to within one struct field of the surface — it has
simply never had a caller.

## What Changes

Everything below the argument layer already exists and is unchanged.

- **New verb `capture unmute <note|#tag|@person|^project>`**, which lifts a mute
  of that scope. Expressed as a `MUTE` event whose `until` is the present
  instant, because the fold already applies mutes in timestamp order so that the
  newest mute touching a note wins, and the projection already reads an elapsed
  `until` as not muted. No new event kind, no new fold arm, no projection
  change.
- **`capture mute <target> until <date>`**, supplying the half of §2.6's
  "permanently or until a stated date" that no syntax could reach.
  `write::mute` already takes `until: Option<u64>` and threads it to every
  scope; `main.rs` passes a hardcoded `None`.
- **`Command::Mute` gains an `until` field**, which is the one structural change
  and the reason neither of the above was expressible.
- **No change to `capture show`.** An earlier draft of this proposal claimed it
  reported every mute as indefinite. It does not: it already prints "until
  <date>" where there is an end and "indefinitely" where there is not. The
  behaviour had simply never been reachable, because no syntax could set an end.
  A test will pin it rather than a change implementing it.
- Desktop and phone are **unchanged in this change**. `ipc::mute` and the FFI
  continue to call `mute_note`, which keeps writing an indefinite mute. §2.6
  says "from any device", so this is a gap that remains open; see Non-goals.

## Capabilities

### New Capabilities

None. This implements requirements that already exist in two specs.

### Modified Capabilities

- `cli`: gains the `unmute` verb and the `mute … until <date>` form. The spec
  has 28 requirements and does not currently mention muting at all, which is
  part of how a command came to contradict two other specs without anything
  noticing.

## Impact

Affected code:

- `apps/capture-cli/src/args.rs` — `Command::Mute` gains `until`; a new `unmute`
  arm; `until <date>` parsing shared by both.
- `apps/capture-cli/src/main.rs` — stops passing a hardcoded `None`.
- `apps/capture-cli/src/write.rs` — a `unmute` entry point beside `mute`, using
  the same `Muted` target enum.
- `apps/capture-cli/tests/` — the reversal and the lapse, neither of which any
  test could reach before.

- `crates/capture-store/src/projection.rs` — three predicates that read a mute
  which has *ended* as still in force. Found while designing, not while
  proposing: ten places read `muted_until`, seven ask whether the end has
  passed and three ask only whether an end exists. One of the three is the
  review selection predicate, which makes it a live defect against §2.6 — "mute
  removes a note from review ... permanently **or until a stated date**" — and
  one that an unmute would otherwise inherit, restoring a note everywhere
  except the surface §2.6 names first.

Unaffected, and deliberately so:

- `capture-core`'s `MuteBody` and the `apply_mutes` fold. Both already do what
  this needs. Changing either would alter what existing stored events mean,
  which §1.3 forbids reaching for when a rebuild would not produce it.
- `PROJECTION_SCHEMA` — no table and no column changes, so no bump.

Risk: an `unmute` is itself a mute event, so on a replica that has not yet seen
it the note stays suppressed. That is ordinary convergence rather than a flaw,
and it is the same behaviour a timed mute already has.

## Non-goals

- **The desktop and phone surfaces.** §2.6 says "reversible from any device",
  and after this change it is reversible from one. Both call `mute_note`
  directly and both need the same two controls. Deferred because the window and
  the phone each want a design decision about where an unmute control lives, and
  bundling three surfaces hides whether the command-line one is right.
- **`mute_note` gaining an `until` parameter.** `mute_scope` and `mute_entity`
  already take one and `mute_note` does not; the CLI will route note mutes
  through `mute_scope` as it already does for tags and date ranges. Unifying the
  three store entry points is a tidy-up that should not ride along with a
  behaviour fix.
- **Re-muting semantics beyond "newest wins".** No stack, no history of what a
  mute used to be. The fold already resolves this and inventing more state would
  be the kind of thing §1.3 makes expensive to undo.
- **A listing of what is currently muted.** Worth having, and a separate read
  command rather than part of making the write reversible.
