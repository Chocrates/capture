//! Kill a process doing real work, then check the store is still coherent.
//!
//! # Written before anything claimed to be crash-safe
//!
//! Every durability decision in this crate — one transaction per append,
//! `synchronous=FULL` on every writing connection, commit-then-clear in the
//! drain, generations rather than renames — is a claim about what survives an
//! interruption. Claims like that are cheap to make and, without something
//! trying to break them, indistinguishable from wishful comments.
//!
//! So this exists, and it is deliberately not a unit test. Killing a thread is
//! not killing a process: within one process nothing can stop destructors
//! running, buffers flushing and transactions closing tidily on the way out,
//! and tidy shutdown is precisely what a crash is not. The victim is therefore
//! a separate binary, removed with `SIGKILL`, which no code can intercept.
//!
//! # What this does not test
//!
//! **This tests process death, not power loss.** A killed process leaves its
//! writes in the kernel's page cache, and the kernel writes them out
//! afterwards, so everything here would pass on a store configured with no
//! durability at all. `synchronous=FULL` protects against the machine losing
//! power with data still in that cache, and nothing in software can simulate
//! that.
//!
//! Stating the gap is the point. A harness whose limits are unstated reads as
//! proof of more than it shows, and "we have a crash test" then becomes the
//! reason nobody looks at the case it never covered.
//!
//! # About the clock allowances below
//!
//! The crate bans reading clocks so that logic which must be reproducible
//! cannot quietly depend on the time of day. This file is the supervisor, not
//! the system under test: it needs a monotonic clock to decide when to kill,
//! and a wall clock to seed its generator. Neither value reaches the store.

// See the module comment: this is the harness, and the ban it opts out of
// exists to keep clocks out of the code it is testing.
#![allow(clippy::disallowed_methods, clippy::disallowed_types)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant, SystemTime};

use brain_core::{body::CaptureBody, envelope::Envelope};
use brain_store::{
    log::Log,
    ops::Ops,
    projection::{self, Store},
    purge, spool,
};

/// How many kill-and-inspect rounds each mode gets.
const ROUNDS: usize = 8;

/// The window kills are drawn from.
///
/// The lower bound is not zero. Every write here goes through an `fsync`, so a
/// victim killed in its first few milliseconds has done nothing but open files,
/// and a harness full of those rounds looks green while testing nothing at all.
/// The upper bound keeps the suite quick.
const MIN_RUN: Duration = Duration::from_millis(60);
const MAX_RUN: Duration = Duration::from_millis(300);

fn victim() -> &'static str {
    env!("CARGO_BIN_EXE_crash-victim")
}

/// A seeded generator, so a failing round can be reproduced from its printed
/// seed rather than re-run hopefully.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn duration(&mut self, lo: Duration, hi: Duration) -> Duration {
        let span = (hi - lo).as_micros() as u64;
        lo + Duration::from_micros(self.next() % span.max(1))
    }
}

fn seed() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x2545F4914F6CDD1D)
        | 1
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let d = std::env::temp_dir().join(format!("brain-crash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// Create the store before any victim runs.
    ///
    /// Creating a store and crashing during it is a real scenario, but it is a
    /// different one: it is about whether a schema appears whole, which
    /// `db::create_schema` is tested for directly. Mixing it in here would mean
    /// rounds that fail because the store was never built, drowning the
    /// failures that are about what a crash did to a store that existed.
    fn with_store(self) -> Scratch {
        Log::open(&self.0.join("events.db")).expect("store is created");
        Ops::open(&self.0.join("ops.db")).expect("local state is created");
        self
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// Start a victim, let it work for a randomised while, then `SIGKILL` it.
///
/// The child is always reaped, including when an invariant check later fails —
/// a leaked victim would keep writing to the store the next round is inspecting
/// and turn one failure into a run of unrelated ones.
fn run_and_kill(mode: &str, root: &Path, rng: &mut Rng) -> Duration {
    let mut cmd = Command::new(victim());
    cmd.arg(mode).arg(root);

    // The purge's interesting window — between the projection being cleared and
    // the payload being destroyed — is microseconds wide. Widening it is the
    // only way a kill lands there often enough to test, and it widens a real
    // window rather than simulating a fake one.
    if mode == "redact" {
        cmd.env("BRAIN_PURGE_PAUSE_MS", "40");
    }

    let mut child: Child = cmd.spawn().expect("victim starts");

    let lifetime = rng.duration(MIN_RUN, MAX_RUN);
    let start = Instant::now();
    while start.elapsed() < lifetime {
        std::thread::sleep(Duration::from_millis(2));
        if let Ok(Some(status)) = child.try_wait() {
            panic!("victim exited on its own before it could be killed: {status}");
        }
    }

    // SIGKILL. Not SIGTERM: a signal the process can handle is a signal it can
    // shut down cleanly on, which is the opposite of what is being tested.
    child.kill().expect("victim can be killed");
    child.wait().expect("victim can be reaped");
    lifetime
}

/// Everything the log says about itself, checked for internal agreement.
fn check_log(root: &Path) -> u64 {
    let log = Log::open_read(&root.join("events.db")).expect("the log still opens");
    let events = log.all().expect("every row still decodes");
    let heads = log.heads().expect("heads are readable");
    let pending = log.pending().expect("pending is readable");

    // Every row is a complete event. A torn write would fail to decode above,
    // so reaching here already establishes it; this states the count for the
    // messages below.
    let total = events.len() as u64;
    assert_eq!(
        total,
        log.count().expect("count"),
        "the row count and the decodable events disagree"
    );

    for (dev, head) in &heads {
        let mut seqs: Vec<u64> = events
            .iter()
            .filter(|e| dev_hex(&e.header.dev) == *dev)
            .map(|e| e.header.seq)
            .collect();
        seqs.sort_unstable();

        // The head is a contiguous head, so everything below it must be
        // present. A gap here is unrecoverable in the real system: a device
        // publishes one number per origin and cannot describe a hole.
        for want in 1..=*head {
            assert!(
                seqs.binary_search(&want).is_ok(),
                "{dev}: head is {head} but sequence {want} is missing — \
                 a gap no summary can describe"
            );
        }

        // And nothing above the head may be silently claimed as held.
        for s in seqs.iter().filter(|s| **s > *head) {
            assert!(
                pending.iter().any(|(d, p)| d == dev && p == s),
                "{dev}: sequence {s} is stored above head {head} without being pending"
            );
        }
    }

    // A stored event that no head accounts for is a row nothing can find.
    for e in &events {
        let dev = dev_hex(&e.header.dev);
        let head = heads.get(&dev).copied().unwrap_or(0);
        assert!(
            e.header.seq <= head || pending.iter().any(|(d, p)| *d == dev && *p == e.header.seq),
            "{dev}: sequence {} is stored but neither covered nor pending",
            e.header.seq
        );
    }

    total
}

fn dev_hex(d: &brain_core::Dev) -> String {
    d.0.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn appending_survives_being_killed() {
    let scratch = Scratch::new("append").with_store();
    let mut rng = Rng(seed());
    let mut last = 0;

    for round in 0..ROUNDS {
        run_and_kill("append", scratch.path(), &mut rng);
        let total = check_log(scratch.path());
        assert!(
            total >= last,
            "round {round}: the log shrank, from {last} to {total}"
        );
        last = total;
    }

    assert!(
        last > 0,
        "no events were ever written — the harness proved nothing"
    );
}

/// Captures the victim was told were safe.
///
/// A final line without its newline is a write that was interrupted, and an
/// interrupted acknowledgement was never an acknowledgement — the victim had
/// not returned from it — so it is dropped rather than demanded.
fn acknowledged(root: &Path) -> Vec<u64> {
    let s = std::fs::read_to_string(root.join("acknowledged")).unwrap_or_default();
    let complete = s.rsplit_once('\n').map(|(head, _)| head).unwrap_or("");
    complete
        .lines()
        .filter_map(|l| l.trim().parse().ok())
        .collect()
}

/// The index the victim stamped into a capture payload.
fn index_of(body: &[u8]) -> Option<u64> {
    let b: CaptureBody = minicbor::decode(body).ok()?;
    Some(u64::from_le_bytes(b.nonce[..8].try_into().ok()?))
}

#[test]
fn a_killed_drain_neither_loses_nor_duplicates_a_capture() {
    // The window this exists for: the drain commits a record and is killed
    // before it can clear the spool.
    //
    // Note what is *not* asserted. A record that is in the log and still in the
    // spool is the expected result of that kill, not a fault — it is what
    // commit-then-clear produces. The ordering is chosen precisely because the
    // alternative produces the opposite overlap, where a capture is in neither
    // place because the spool was cleared before the commit landed.
    //
    // Detecting that requires knowing what *should* be there, which is why the
    // victim numbers its captures consecutively and stamps the number into each
    // payload. Loss shows up as a gap in the numbering. Inspecting only what
    // survived could never find it: a capture that vanished leaves nothing to
    // inspect, and every remaining record looks perfectly well-formed.
    let scratch = Scratch::new("drain").with_store();
    let mut rng = Rng(seed());
    let mut highest = 0u64;

    for round in 0..ROUNDS {
        run_and_kill("drain", scratch.path(), &mut rng);
        check_log(scratch.path());

        let log = Log::open_read(&scratch.path().join("events.db")).unwrap();
        let events: Vec<Envelope> = log.all().unwrap();

        // No payload is committed twice. This is the claim that
        // commit-then-clear plus body-hash recognition makes.
        let hashes: BTreeSet<[u8; 32]> = events.iter().map(|e| e.header.bhash.0).collect();
        assert_eq!(
            hashes.len(),
            events.len(),
            "round {round}: a capture was committed more than once"
        );

        let mut present: BTreeSet<u64> = events
            .iter()
            .filter_map(|e| e.body.as_deref().and_then(index_of))
            .collect();
        for rec in spool::read_all(&scratch.path().join("spool")).unwrap() {
            if let Some(i) = index_of(&rec.payload) {
                present.insert(i);
            }
        }

        // Every capture the victim was told was safe must still exist
        // somewhere. This is the check that a gap analysis cannot replace:
        // captures are lost from the top of the numbering, so the survivors
        // stay perfectly consecutive while the losses leave no trace at all.
        for want in acknowledged(scratch.path()) {
            assert!(
                present.contains(&want),
                "round {round}: capture {want} was acknowledged and is now in \
                 neither the log nor the spool"
            );
            highest = highest.max(want);
        }
    }

    assert!(
        highest > 0,
        "nothing was ever spooled or drained — the harness proved nothing"
    );
}

#[test]
fn a_killed_purge_never_leaves_content_half_destroyed() {
    // The purge writes across two databases with no transaction spanning them,
    // and nothing in the log distinguishes an interrupted purge from a finished
    // one — the redaction event reads identically either way. The intent marker
    // is the only thing that can tell them apart, and this is what kills a
    // process in the middle to find out whether it does.
    //
    // The dangerous direction is specific: content gone from the log but still
    // in the search index, which is the one place that hands it back on a
    // query. The ordering exists to make that impossible, so it is what gets
    // asserted.
    let scratch = Scratch::new("purge").with_store();
    let mut rng = Rng(seed());
    let mut ever_purged = false;

    for round in 0..ROUNDS {
        run_and_kill("redact", scratch.path(), &mut rng);
        check_log(scratch.path());

        let ops = Ops::open_read(&scratch.path().join("ops.db")).expect("local state opens");
        let unfinished = ops.unfinished_purges().expect("markers are readable");

        // Finish whatever was interrupted, exactly as a real startup would.
        let mut log = Log::open(&scratch.path().join("events.db")).expect("the log opens");
        let generation = Ops::open(&scratch.path().join("ops.db"))
            .unwrap()
            .generation()
            .unwrap();
        let state_path = projection::generation_path(&scratch.path().join("state.db"), generation);
        if !state_path.exists() {
            continue;
        }
        let mut state = Store::open(&state_path).expect("the projection opens");
        let ops_w = Ops::open(&scratch.path().join("ops.db")).unwrap();
        purge::resume(&mut log, &mut state, &ops_w, 1_800_000_000_000)
            .expect("an interrupted purge can always be finished");

        assert!(
            ops_w.unfinished_purges().unwrap().is_empty(),
            "round {round}: a purge could not be finished"
        );

        // Now the invariant. Every capture whose payload is gone must also be
        // gone from the projection and from the search index.
        for e in log.all().unwrap() {
            if e.header.kind != brain_core::event::Kind::CAPTURE || e.body.is_some() {
                continue;
            }
            ever_purged = true;
            let tgt = e.header.id();
            assert!(
                state.text_of(&tgt).unwrap().is_none(),
                "round {round}: a destroyed payload is still readable in the projection"
            );
        }

        // The other half, and the one that actually depends on the marker: a
        // note the projection reports as destroyed must have no payload left in
        // the log. A crash between purging the projection and destroying the
        // body leaves exactly that state, and nothing in the log describes it —
        // so if `resume` did not find it, the content survives forever behind a
        // note that says it is gone.
        for n in state.timeline(10_000).unwrap() {
            if !n.is_redacted() {
                continue;
            }
            let survivors: Vec<_> = log
                .all()
                .unwrap()
                .into_iter()
                .filter(|e| e.header.kind == brain_core::event::Kind::CAPTURE)
                .filter(|e| e.header.id() == n.tgt && e.body.is_some())
                .collect();
            assert!(
                survivors.is_empty(),
                "round {round}: {} reports its content destroyed, and the payload is still there",
                n.slug
            );
        }

        // And nothing findable has lost its payload — the same statement from
        // the other side, which is the one that catches an index left behind.
        for hit in state.search("capture", 1_800_000_000_000).unwrap() {
            let held: Vec<_> = log
                .all()
                .unwrap()
                .into_iter()
                .filter(|e| e.header.id() == hit)
                .collect();
            for e in held {
                assert!(
                    e.body.is_some(),
                    "round {round}: content is findable but its payload is destroyed"
                );
            }
        }

        let _ = unfinished;
    }

    assert!(
        ever_purged,
        "nothing was ever destroyed — the harness proved nothing"
    );
}

#[test]
fn a_killed_rebuild_leaves_a_usable_projection() {
    // A rebuild writes a new generation beside the old one and then moves the
    // pointer. Killed at any point, the pointer must still name a projection
    // that opens — possibly the previous one, never a half-written one and
    // never a file that is not there.
    let scratch = Scratch::new("rebuild").with_store();
    let mut rng = Rng(seed());
    let mut saw_a_rebuild = false;

    for round in 0..ROUNDS {
        run_and_kill("rebuild", scratch.path(), &mut rng);
        check_log(scratch.path());

        let ops = Ops::open_read(&scratch.path().join("ops.db"));
        let Ok(ops) = ops else { continue };
        let generation = ops.generation().expect("the pointer is readable");
        if generation == 0 {
            continue;
        }
        saw_a_rebuild = true;

        let path = projection::generation_path(&scratch.path().join("state.db"), generation);
        assert!(
            path.exists(),
            "round {round}: the pointer names generation {generation}, which is not there"
        );
        let store = Store::open_read(&path).unwrap_or_else(|e| {
            panic!("round {round}: generation {generation} does not open: {e}")
        });
        store
            .note_count()
            .unwrap_or_else(|e| panic!("round {round}: generation {generation} is unusable: {e}"));
    }

    assert!(
        saw_a_rebuild,
        "no rebuild ever completed — the harness proved nothing"
    );
}
