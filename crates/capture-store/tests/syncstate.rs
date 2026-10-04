//! Whether sync is running, which is a different question from whether a peer
//! was reached.
//!
//! The case these exist for is in the owner's own ledger, written as a question
//! because nothing would answer it:
//!
//! ```text
//! ## Open
//! - Phone last reached by desktop 51h ago — sync not running?
//! ```
//!
//! It was not running. `capture-syncd` had never been started on that machine,
//! correctly — `install.sh` declines to open a listening socket on anybody's
//! behalf. What was wrong is that the only thing the desktop said was "device
//! d4389d6f has never been reached", which is a statement about a peer and
//! reads as a statement about the network. The remedy for one is to change the
//! network and for the other it is to run a daemon, and the surface could not
//! tell them apart.

use capture_store::StorePaths;
use capture_store::ops::Ops;
use capture_store::syncstate::{SyncState, sync_state};

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let p = std::env::temp_dir().join(format!(
            "capture-syncstate-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        capture_store::store::create(&StorePaths::under(p.clone()), 1_700_000_000_000).unwrap();
        Scratch(p)
    }
    fn paths(&self) -> StorePaths {
        StorePaths::under(self.0.clone())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const DAY: u64 = 24 * 60 * 60 * 1000;
const NOW: u64 = 1_800_000_000_000;

fn peer(n: u8) -> [u8; 32] {
    [n; 32]
}

#[test]
fn one_device_is_not_failing_to_sync() {
    // Nothing paired is not a failure, and saying so on every single-device
    // archive is how a reader learns to skip the section.
    let s = Scratch::new("alone");
    let ops = Ops::open(&s.paths().ops).unwrap();
    assert_eq!(sync_state(&ops, 0, NOW).unwrap(), SyncState::Alone);
    assert_eq!(SyncState::Alone.message(), None);
}

#[test]
fn a_paired_device_whose_daemon_never_ran_says_so() {
    // The owner's case, reproduced: a peer this device knows about, and no
    // round ever. `peer_missed` is how a peer comes to exist with no
    // `last_seen` — a dial that was attempted and failed, which is exactly what
    // happens when there is a roster and nothing running on the other end.
    let s = Scratch::new("neverran");
    let ops = Ops::open(&s.paths().ops).unwrap();
    ops.peer_missed(&peer(1)).unwrap();

    assert_eq!(sync_state(&ops, 1, NOW).unwrap(), SyncState::NeverRan);

    let (tier, text) = sync_state(&ops, 1, NOW).unwrap().message().expect("said");
    assert_eq!(tier, "attention");
    assert!(
        text.contains("capture-syncd"),
        "it should name the remedy: {text}"
    );
    assert!(
        !text.contains("network"),
        "this is not a network problem and must not read as one: {text}"
    );
}

#[test]
fn a_round_that_ran_and_reached_nobody_is_about_the_network() {
    // The other half of the distinction. Same peer rows, different cause,
    // different remedy.
    let s = Scratch::new("nonetwork");
    let ops = Ops::open(&s.paths().ops).unwrap();
    ops.peer_missed(&peer(1)).unwrap();
    ops.round_ran(NOW, true, None).unwrap();

    assert_eq!(
        sync_state(&ops, 1, NOW).unwrap(),
        SyncState::NeverReachedAnyone
    );
    let (_, text) = sync_state(&ops, 1, NOW).unwrap().message().expect("said");
    assert!(
        !text.contains("capture-syncd"),
        "the daemon is running; pointing at it would send somebody the wrong way: {text}"
    );
}

#[test]
fn the_job_is_asked_about_before_the_network() {
    // Both are wrong at once: no round for days *and* no peer ever reached.
    // The round question wins, because if rounds are not happening then what
    // the network would have done is not a question anybody can act on.
    let s = Scratch::new("both");
    let ops = Ops::open(&s.paths().ops).unwrap();
    ops.peer_missed(&peer(1)).unwrap();
    ops.round_ran(NOW - 5 * DAY, true, None).unwrap();

    match sync_state(&ops, 1, NOW).unwrap() {
        SyncState::NotRunning { since_ms } => {
            assert!(since_ms >= 5 * DAY - 1000, "{since_ms}");
        }
        other => panic!("the job comes first, got {other:?}"),
    }
}

#[test]
fn a_recent_round_that_reached_somebody_says_nothing() {
    let s = Scratch::new("working");
    let ops = Ops::open(&s.paths().ops).unwrap();
    ops.peer_answered(&peer(1), NOW).unwrap();
    ops.round_ran(NOW, true, None).unwrap();

    assert_eq!(sync_state(&ops, 1, NOW).unwrap(), SyncState::Working);
    assert_eq!(sync_state(&ops, 1, NOW).unwrap().message(), None);
}

#[test]
fn a_day_of_quiet_is_ordinary_and_four_is_not() {
    // The thresholds, which are a judgement: the period is fifteen minutes and
    // Doze stretches it to hours, so under a day is noise. A day is still
    // ordinary — the other device was off. Four days is not.
    let s = Scratch::new("thresholds");
    let ops = Ops::open(&s.paths().ops).unwrap();
    ops.round_ran(NOW, true, None).unwrap();

    ops.peer_answered(&peer(1), NOW - 2 * DAY).unwrap();
    let (tier, _) = sync_state(&ops, 1, NOW).unwrap().message().expect("said");
    assert_eq!(tier, "worth-knowing", "two days is worth knowing");

    ops.peer_answered(&peer(1), NOW - 4 * DAY).unwrap();
    let (tier, _) = sync_state(&ops, 1, NOW).unwrap().message().expect("said");
    assert_eq!(tier, "attention", "four days wants a decision");
}

#[test]
fn a_device_paired_and_never_dialled_is_not_silence() {
    // Membership comes from the roster, not from `ops`. A `peer_state` row
    // appears once somebody has been dialled or has answered — so a device
    // paired and never yet contacted has no row at all, and inferring "is
    // anything paired" from `ops` would answer the case this exists for with
    // silence. The first draft did exactly that.
    let s = Scratch::new("undialled");
    let ops = Ops::open(&s.paths().ops).unwrap();
    // No `peer_missed`, no `peer_answered`: nothing in `ops` at all.
    assert!(ops.peer_addresses().unwrap().is_empty());

    assert_eq!(
        sync_state(&ops, 1, NOW).unwrap(),
        SyncState::NeverRan,
        "one paired device and no round is the case to report, row or no row"
    );
    assert_eq!(
        sync_state(&ops, 0, NOW).unwrap(),
        SyncState::Alone,
        "and with nothing paired it is still silent"
    );
}

#[test]
fn it_is_answerable_from_a_read_only_handle() {
    // §8.8, which is the whole reason this reads `ops` rather than asking the
    // writer: the moment this most needs answering is the moment something is
    // wrong, and a diagnostic that stops working then is one nobody can use.
    let s = Scratch::new("readonly");
    {
        let ops = Ops::open(&s.paths().ops).unwrap();
        ops.peer_missed(&peer(1)).unwrap();
    }
    let ops = Ops::open_read(&s.paths().ops).unwrap();
    assert_eq!(sync_state(&ops, 1, NOW).unwrap(), SyncState::NeverRan);
}
