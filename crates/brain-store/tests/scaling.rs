//! Does a write get slower as the archive grows?
//!
//! Ignored by default and kept because the answer was yes, three times over,
//! and each one was invisible until the one before it was gone:
//!
//!     events        first    after chain_tip   after known_names
//!       210        21.7 ms        23.4 ms            20.6 ms
//!     1 010        26.6           23.3               22.3
//!     3 010        35.4           25.8               23.2
//!     8 010        61.5           34.1               23.3
//!
//! Three reads of the whole archive on the path a write waits on: the
//! incremental fold filtering every event in memory, `chain_tip` taking a
//! maximum over all of them — once per event *authored*, so a capture paid
//! twice — and `known_names` building a map of every slug and alias to answer
//! at most a handful of questions.
//!
//! A capture does not block the gesture, which is why none of this showed up in
//! the latency histogram. `brain +` in a terminal waits for all of it.
// Reading a clock, which the purity lint otherwise refuses. Timing a write is
// not stamping an event, and the boundary that rule protects is the fold's.
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "measuring how long a write takes, not stamping an event"
)]

use brain_store::StorePaths;
use brain_store::session::Session;
use std::time::Instant;

#[test]
#[ignore = "a measurement, not a check"]
fn a_write_against_a_growing_log() {
    let dir = std::env::temp_dir().join(format!("brain-scale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = StorePaths::under(dir.clone());
    brain_store::store::create(&p, 1_700_000_000_000).unwrap();

    let mut s = Session::open_for_capture(p.clone()).unwrap();
    let mut n = 0usize;
    for target in [200usize, 1000, 3000, 8000] {
        while n < target {
            let body = brain_store::session::capture_body(&format!("note {n}")).unwrap();
            s.capture(&body).unwrap();
            n += 1;
        }
        // Ten more, timed.
        let t = Instant::now();
        for i in 0..10 {
            let body = brain_store::session::capture_body(&format!("timed {n} {i}")).unwrap();
            s.capture(&body).unwrap();
        }
        n += 10;
        println!(
            "{:>5} events: {:.1} ms per capture",
            s.log.count().unwrap(),
            t.elapsed().as_secs_f64() * 100.0
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
