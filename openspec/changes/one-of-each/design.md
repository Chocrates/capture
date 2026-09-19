## Context

Five facts are written down between two and nine times each, counted in the
proposal. Two of the copies already disagree.

The interesting part is not the count but the mechanism. Almost all of these
appeared recently, and each was added by copying a nearby example rather than
looking for an owner — the overlay needed a local zone, the command line had
one twelve lines from where the author was already reading, and that was that.
A plan that only removes today's copies leaves the mechanism running.

## Goals / Non-Goals

**Goals:**

- One definition per fact, in the place a reader would look for it.
- The disagreements resolved deliberately rather than by whichever copy a
  refactor happened to keep.
- A guard that makes the next copy fail rather than land.

**Non-Goals:**

Deduplicating explanatory prose; merging the `capture_body_*` constructors;
unifying the two surface record types; a utility crate.

## Decisions

### Each thing moves to what it is about, not to a drawer

`local_zone` goes to `capture-store`'s `notifier`, because the only reason the
system reads a zone at this layer is to decide what hour it is for quiet hours
and batches. The short key and the hex pair go to `capture-core`, beside the
key type.

The alternative — one `util` or `common` module holding all five — is worse in
a specific way rather than an aesthetic one: it has no subject, so nothing can
be argued about its contents, and the next unowned helper lands there too. A
drawer is how you get a drawer.

### The disagreement is resolved toward the stricter copy, and written down

`unhex` exists twice and they differ on the empty string: one returns `None`,
the other returns an empty vector. The strict one wins, because both callers
sit either side of §5.3's two-signature readmission, and a readmission request
that decodes to nothing should be refused rather than carried forward as a
key of length zero.

That reasoning goes in the surviving function. A merge that silently kept one
behaviour would leave the next reader unable to tell a decision from an
accident.

### Opening the current generation read-only becomes one call

The expression

```rust
Store::open_read(&generation_path(&paths.state, Ops::open_read(&paths.ops)?.generation()?))
```

appears around twenty times. It is not merely long: it encodes the rule that
*the current generation is whatever `ops` says it is*, and a call site that
opened a stale path instead would read an archive that is real, consistent and
out of date — which is the hardest kind of wrong to see.

### The guard extends `unwired`

`tools/unwired.py` already walks the workspace refusing a public item nothing
calls. The same walk can refuse a second definition of a name already defined
elsewhere, with an allow-list for the genuine cases — `new`, `open`, `fmt` and
the other names a type is expected to have.

This is the part that matters more than the cleanup. Without it, the next
person needing a local zone copies the nearest one, exactly as happened four
times already.

## Risks / Trade-offs

- **A shared test fixture couples nine suites** → They already share the
  behaviour; what they do not share is the bug fixed in one of them. The
  fixture stays deliberately small — create a store, hand back paths, delete
  on drop — and anything a suite needs beyond that stays in that suite.

- **`capture-core` is `no_std`** → Key rendering and hex must stay allocation-
  aware and dependency-free. Both are a dozen lines of `format!` over a slice,
  so this is a constraint to respect rather than an obstacle.

- **Moving `local_zone` into `capture-store` puts an environment read in a
  crate that avoids the outside world** → It is already there. `notify` shells
  out to `notify-send` from the same module, and `deliver_alerts` reads the
  clock. The boundary §8.1 draws is around *folding*, which stays pure; the
  notifier is explicitly the place effects live.

- **The guard will produce false positives** → Hence the allow-list. A guard
  that cannot be satisfied gets disabled, which is worse than no guard.

## Open Questions

1. **Does the dedupe guard belong in `make ci` or only in `make accept`?** It
   is cheap, but a check that fails on a legitimate second `fn new` while
   somebody is mid-refactor is a check they will start passing `--no-verify`
   to. Worth trying in `accept` first.

2. **Is the `Scratch` fixture worth sharing across crates, or only within
   `capture-store`?** `capture-ffi`'s tests have their own and it differs —
   it opens an `Archive` rather than a store. Probably two fixtures, honestly
   named, rather than one with a flag.
