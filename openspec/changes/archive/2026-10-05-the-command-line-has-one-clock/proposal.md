# the-command-line-has-one-clock

## Why

Fourteen places in the command line read the wall clock directly. Fourteen
chances to read it differently, and two of them already differed about whether
the answer was an `i64` or a `u64`. It is the same argument as one
`peer_is_stale`: "what time is it" answered once cannot be answered two ways.

Nothing can move it either, so what the command line *interprets* — what "today"
is for the agenda and the overdue bands, where a `split` boundary falls, how
long ago a peer was reached — is only ever exercised at whatever moment the
suite happens to run.

**What this does not unblock, having checked rather than assumed.** I expected
this to reach `capture recover`, the redaction window and snapshot expiry. It
does not: those are the *store's* questions, asked with the store's own clock
(`session::now_ms`), and the command line's clock does not touch it. A capture
made with the override set to 2030 lands on today's timeline, which is how this
was settled. Moving the store's clock is a larger decision, because that one
would reach the event stamps — recorded for you rather than taken here.

## What Changes

- One `now_ms()`, and every site reads the clock through it. Fourteen became
  one, which is worth doing on its own: "what time is it" answered the same way
  everywhere is the same argument as one `peer_is_stale`.
- `CAPTURE_NOW_MS` moves it, **in debug builds only**.
- A test for `capture recover` end to end, which is the thing this unblocks
  first and the reason the gap was worth closing.

## Capabilities

### Modified Capabilities

- `cli` — the clock is one function, and a test can move it.

## Non-goals

- **Not a release capability.** The override is compiled out of a release
  binary. See `design.md`: a faked clock is not like a faked path, because it
  stamps events.
- **No change to what any command does.** Every site reads the same clock it
  read before; it now reads it through one function.
- **Not the store, the fold or the daemon.** They take time as an argument
  already, which is why they are testable and the command line is not. This
  gives the command line the same property by the only means available to a
  binary that reads a global clock.
