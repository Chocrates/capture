//! Draining work that has become due, from the process that is already running.
//!
//! # The half of the rule that was never built
//!
//! `brain_store::due` describes two moments where the queue is drained: every
//! process startup, which is the floor, and *a slow tick in the resident
//! overlay*, which is what makes a deadline land near its time rather than at
//! the next time somebody happens to run a command.
//!
//! Only the first existed. The overlay ran all day and never opened a session,
//! so a machine with the overlay running and nothing else touching the store
//! left redactions sitting past their windows — every interface reporting them
//! queued, nothing carrying them out, and no error anywhere. The floor held;
//! the promptness did not.
//!
//! # Why a thread rather than the event loop
//!
//! The overlay's event loop is deliberately asleep between keypresses (see
//! `window.rs`), and waking it every minute to check a queue would undo that.
//! This sleeps on its own thread and touches nothing the loop owns.
//!
//! # Why the lock is taken and released each time
//!
//! §8.8 elects one writer. The overlay is the natural holder, but holding the
//! lock *for its lifetime* would mean the terminal chord and every `brain`
//! command failed for as long as the overlay was running — which is always.
//!
//! So the tick takes the lock, drains, and puts it down. When something else
//! holds it, the tick does nothing at all and says nothing: that process is
//! itself a startup, and startups drain. Contention here is not a failure, it
//! is the other half of the rule doing the work.

use std::time::Duration;

use brain_store::StorePaths;
use brain_store::session::Session;

/// How often to look.
///
/// A minute is chosen against the two deadlines this actually serves. A
/// redaction window is thirty minutes, so a minute is invisible against it. An
/// intent set for 3pm firing at 3:00:59 is on time in any sense a person means
/// (§2.4).
///
/// Faster would buy nothing: this is a display latency, not a correctness
/// bound, and the floor already guarantees the work is never lost. Slower would
/// start to make a reminder feel like it had not fired.
const EVERY: Duration = Duration::from_secs(60);

/// The interval, overridable for tests.
///
/// Only ever read at startup, and only ever shortened — a test that had to wait
/// a minute to find out whether a tick fires would not be run.
fn interval() -> Duration {
    std::env::var("BRAIN_DUE_TICK_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_millis)
        .unwrap_or(EVERY)
}

/// Drain the due queue on a timer, for as long as the process lives.
///
/// Never returns. Spawned once, from `run`.
pub fn tick(paths: StorePaths) -> ! {
    let every = interval();
    loop {
        std::thread::sleep(every);

        // A failure to open is somebody else writing. They took the lock, which
        // means they started, which means they drained — so there is nothing to
        // do and nothing worth saying about it. Contention here is the other
        // half of the rule doing the work, not an error.
        if let Ok(mut s) = Session::open(paths.clone()) {
            match s.drain_due() {
                Ok(d) if d.any() => {
                    tracing::info!(
                        destroyed = d.destroyed,
                        expired = d.expired,
                        "overdue work completed on the tick"
                    );
                }
                Ok(_) => {}
                Err(e) => tracing::warn!(%e, "due work could not be processed"),
            }
        }
    }
}
