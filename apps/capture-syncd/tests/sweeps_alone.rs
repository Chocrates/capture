//! The daemon reads captures on a device with no peers.
//!
//! The first test in this crate, for a regression that actually shipped. The
//! sweep used to reach a store only through `round::reconcile`, and
//! `round::tick` gives up before that whenever there is nothing to sync with —
//! no roster secret, no members, nothing reachable. So promotion was a side
//! effect of *having a peer*, and a single device with this daemon running was
//! swept never: you captured a dated task, asked for your tasks, and were told
//! there were none.
//!
//! The fix was to move the sweep above the peer phase, and its comment says so.
//! Nothing checked it, in a crate with no tests at all.
//!
//! # Why this spawns the daemon rather than calling a function
//!
//! The defect was never in a function. Both halves were correct on their own —
//! the sweep worked, and `round::tick` was right to give up with nobody to talk
//! to. What was wrong was the *order* of two things in a loop, and that is only
//! observable by running the loop.

// Waiting for another process to get round to something, which needs a clock
// that is not the archive's. Nothing here stamps an event — the boundary that
// rule protects is the fold's, and this is a test watching a daemon.
#![allow(
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "waiting on a spawned process, not stamping an event"
)]

use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Store(std::path::PathBuf);

impl Drop for Store {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_device_with_no_peers_still_has_its_captures_read() {
    let dir = std::env::temp_dir().join(format!("capture-syncd-alone-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store(dir.clone());
    let paths = capture_store::StorePaths::under(dir.clone());
    capture_store::store::create(&paths, 1_700_000_000_000).unwrap();

    // A capture that will promote once something reads it, and deliberately
    // not settled here — the daemon is the thing under test.
    {
        let mut s = capture_store::session::Session::open_for_capture(paths.clone()).unwrap();
        let body = capture_store::session::capture_body("pay the invoice 2026-09-20").unwrap();
        s.capture(&body).unwrap();
    }
    assert!(
        unread(&paths) > 0,
        "the capture was already read, so this would prove nothing"
    );

    // A port of its own. `CAPTURE_SYNC_PORT` exists so a daemon can run beside
    // a real one, and without it this test would fight whatever is already
    // listening on the machine it runs on.
    let port = 40_000 + (std::process::id() % 20_000) as u16;
    let _daemon = Daemon(
        Command::new(env!("CARGO_BIN_EXE_capture-syncd"))
            .env("CAPTURE_HOME", &dir)
            .env("CAPTURE_SYNC_PORT", port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the daemon starts"),
    );

    // Polled rather than slept: the sweep runs at the top of the first
    // iteration, so this usually settles in well under a second, and a fixed
    // wait would be both slower and more likely to be flaky on a loaded machine.
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if unread(&paths) == 0 {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    drop(store);
    panic!("the daemon ran and never read the capture — the sweep is behind the peer phase again");
}

/// How many captures have not been read.
fn unread(paths: &capture_store::StorePaths) -> usize {
    // Read-only, so asking does not itself sweep — opening a session would
    // catch the store up and answer its own question.
    match capture_store::projection::current(paths) {
        Ok(store) => store
            .unread_count(capture_parse::GRAMMAR_VERSION)
            .unwrap_or(usize::MAX),
        // Before the daemon's first open there may be no generation yet.
        Err(_) => usize::MAX,
    }
}
