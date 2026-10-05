# Design

## Why an environment variable, and why only in debug

The store, the fold and the daemon all take time as an argument. That is why
they are testable and why this binary is not: a command is invoked by somebody
typing it, and there is no argument list to thread a clock through that would
not also appear in `capture help`.

So the clock is read from the environment, which is how this binary already
learns where its archive is — and the precedent inside it is
`CAPTURE_PURGE_PAUSE_MS`, a hook the crash harness sets to widen a window.

That hook ships unguarded. This one must not, and the difference is worth
stating rather than assuming:

A faked `CAPTURE_HOME` points a command at the wrong archive, and everything it
then says is about that archive — wrong, and visibly wrong. A faked clock is
different in kind. It does not only change what a command *reports*: captures
are stamped with it, the HLC advances from it, and a redaction's window is
measured from it. An archive whose event ordering came from an environment
variable is damaged in a way that looks like nothing at all, and the damage
syncs.

`cfg(debug_assertions)` is the line. Tests run the debug binary — that is what
`CARGO_BIN_EXE_capture` is — and an installed release ignores the variable
entirely, because the code reading it is not compiled.

## One function, fourteen call sites

The consolidation is worth as much as the hook. Fourteen places reading the
clock directly is fourteen chances to read it differently — one of them already
does `as u64` where another does `as i64`, and `backup.rs` formats a timestamp
for a filename while the rest take milliseconds.

It is the same argument as `peer_is_stale`: "what time is it" answered in one
place cannot be answered two ways. And it is what makes the hook work at all —
an override honoured by thirteen sites and missed by the fourteenth would be
worse than none, because a test would pass while one command quietly used the
real clock.

## What this reaches, and what it does not

Checked by running it, after the first draft of this design claimed it would
unblock `capture recover`.

It reaches everything the command line decides for itself: "today" for the
agenda and the overdue bands, a `split` boundary, the age of a peer.

It does not reach the drain. Whether a redaction's window has passed is asked
by the store with `session::now_ms`, and nothing here touches that — so
`capture recover`, snapshot expiry and the destruction itself remain out of
reach. A capture made with `CAPTURE_NOW_MS` set to 2030 appears on today's
timeline, because the store stamped it.

That also narrows the danger. The first draft of the doc comment said a faked
clock advances the HLC and damages an archive's ordering. It does not — the
stamps are the store's. It can still put a series split at a boundary nobody
chose, which writes an event, so the guard stays; it is simply justified by a
smaller harm than I first wrote.
## Moving the store's clock is the larger question

`capture recover`, which no test has ever invoked. A snapshot exists only after
a redaction has been carried out, and that needs the window to have passed. With
a clock the test can order the destruction, move past the window, let the drain
run, and then ask what can be recovered — the sequence a person performs over
thirty minutes, in milliseconds.

The same handle reaches snapshot expiry, the overdue boundary and the review
intervals. Those are not done here; the point is that they become possible.
