//! How a reader notices that something changed.
//!
//! # Polling, on purpose
//!
//! The writer could push a notification over its socket, and eventually it
//! will. That is a latency improvement, not a correctness mechanism, and the
//! difference decides where the work goes: a reader that depends on being told
//! shows stale data forever if the message is dropped, the socket is not
//! connected yet, or the writer died between committing and notifying. A reader
//! that looks for itself is at worst late.
//!
//! So the durable answer is a poll, and any push added later is an optimisation
//! layered on top of it — never a replacement for it.
//!
//! # Two different things move
//!
//! Watching the log alone is not enough. A rebuild leaves the log untouched and
//! replaces the projection: same heads, entirely new file. A reader that only
//! watched heads would keep reading a generation nobody writes to any more,
//! showing data that is not wrong so much as frozen.
//!
//! So a change is either of:
//!
//! - **heads moved** — new events; refold or re-query.
//! - **the generation moved** — the projection was rebuilt; reopen the file.
//!
//! Both are cheap to check: one integer per origin, and one integer.

use std::collections::BTreeMap;
use std::time::Duration;

use crate::{Result, log::Log, ops::Ops};

/// How often a reader looks.
///
/// Short enough that a capture on another device appears without feeling
/// delayed, long enough that an idle machine is doing nothing measurable. It is
/// a display latency, not a correctness bound — nothing goes wrong if a poll is
/// missed, it just happens later.
pub const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// What moved since the last look.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Change {
    /// New events are in the log.
    pub events: bool,
    /// The projection was rebuilt; the open file is no longer the current one.
    pub generation: bool,
}

impl Change {
    /// Whether anything at all moved.
    pub fn any(&self) -> bool {
        self.events || self.generation
    }
}

/// A reader's memory of what it has already seen.
pub struct Watch {
    heads: BTreeMap<String, u64>,
    generation: u64,
}

impl Watch {
    /// Start watching from where things stand now.
    pub fn start(log: &Log, ops: &Ops) -> Result<Self> {
        Ok(Watch {
            heads: log.heads()?,
            generation: ops.generation()?,
        })
    }

    /// Look once.
    ///
    /// Reports what moved and remembers it, so a change is reported exactly
    /// once. Both reads are through ordinary read-only connections and neither
    /// needs the writer to be running — a reader that stops noticing changes
    /// when the writer is unhealthy is a reader that shows stale data at
    /// precisely the moment being right matters.
    pub fn poll(&mut self, log: &Log, ops: &Ops) -> Result<Change> {
        let heads = log.heads()?;
        let generation = ops.generation()?;

        let change = Change {
            events: heads != self.heads,
            generation: generation != self.generation,
        };

        self.heads = heads;
        self.generation = generation;
        Ok(change)
    }

    /// The heads this reader has accounted for.
    pub fn heads(&self) -> &BTreeMap<String, u64> {
        &self.heads
    }

    /// The generation this reader has open.
    pub fn generation(&self) -> u64 {
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::testing;
    use std::path::PathBuf;

    struct Bench {
        dir: PathBuf,
        log: Log,
        ops: Ops,
    }

    fn bench(name: &str) -> Bench {
        let dir = std::env::temp_dir().join(format!("brain-watch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Bench {
            log: Log::open(&dir.join("events.db")).unwrap(),
            ops: Ops::open(&dir.join("ops.db")).unwrap(),
            dir,
        }
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    #[test]
    fn an_idle_store_reports_nothing() {
        let b = bench("idle");
        let mut w = Watch::start(&b.log, &b.ops).unwrap();
        assert!(!w.poll(&b.log, &b.ops).unwrap().any());
        assert!(!w.poll(&b.log, &b.ops).unwrap().any());
    }

    #[test]
    fn an_appended_event_is_noticed() {
        let mut b = bench("append");
        let mut w = Watch::start(&b.log, &b.ops).unwrap();

        let mut d = testing::Origin::new(0);
        let e = d.author(
            brain_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "something new"),
            1_700_000_000_000,
        );
        b.log.append(&e).unwrap();

        let c = w.poll(&b.log, &b.ops).unwrap();
        assert!(c.events);
        assert!(!c.generation);
    }

    #[test]
    fn a_change_is_reported_once() {
        // A reader that re-reported the same change every poll would rebuild
        // its view twice a second forever.
        let mut b = bench("once");
        let mut w = Watch::start(&b.log, &b.ops).unwrap();

        let mut d = testing::Origin::new(0);
        let e = d.author(
            brain_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "once"),
            1_700_000_000_000,
        );
        b.log.append(&e).unwrap();

        assert!(w.poll(&b.log, &b.ops).unwrap().events);
        assert!(!w.poll(&b.log, &b.ops).unwrap().events);
    }

    #[test]
    fn a_rebuild_is_noticed_even_though_the_log_did_not_move() {
        // The case watching heads alone would miss entirely: the reader would
        // hold a generation nobody writes to and show frozen data indefinitely.
        let b = bench("rebuild");
        let mut w = Watch::start(&b.log, &b.ops).unwrap();

        b.ops.set_generation(w.generation() + 1).unwrap();

        let c = w.poll(&b.log, &b.ops).unwrap();
        assert!(c.generation, "the projection was replaced");
        assert!(!c.events, "and the log did not move");
        assert!(c.any());
    }

    #[test]
    fn both_can_move_at_once() {
        let mut b = bench("both");
        let mut w = Watch::start(&b.log, &b.ops).unwrap();

        let mut d = testing::Origin::new(0);
        let e = d.author(
            brain_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "both"),
            1_700_000_000_000,
        );
        b.log.append(&e).unwrap();
        b.ops.set_generation(7).unwrap();

        let c = w.poll(&b.log, &b.ops).unwrap();
        assert!(c.events && c.generation);
    }

    #[test]
    fn watching_works_through_read_only_connections() {
        // Diagnostics and views must keep working when the writer is not
        // running, which is exactly when a reader most needs to be honest about
        // what it can see.
        let mut b = bench("read-only");
        let mut d = testing::Origin::new(0);
        let e = d.author(
            brain_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "written by someone else"),
            1_700_000_000_000,
        );
        b.log.append(&e).unwrap();

        let rl = Log::open_read(&b.dir.join("events.db")).unwrap();
        let ro = Ops::open_read(&b.dir.join("ops.db")).unwrap();
        let mut w = Watch::start(&rl, &ro).unwrap();
        assert!(!w.poll(&rl, &ro).unwrap().any());

        let e2 = d.author(
            brain_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(2, "and another"),
            1_700_000_001_000,
        );
        b.log.append(&e2).unwrap();
        assert!(w.poll(&rl, &ro).unwrap().events);
    }
}
