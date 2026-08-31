//! The sync daemon.
//!
//! # Why this is its own process
//!
//! The resident overlay is what the capture gesture waits on, and §8.10
//! freezes that gesture's latency for this milestone. A blocking accept, a
//! peer that stalls halfway through a transfer, an mDNS responder thread —
//! none of it can delay the chord if none of it runs in that process.
//!
//! # Why it is not a second writer
//!
//! §8.8 elects one writer. The overlay does not hold that election
//! continuously: it takes the writer lock per operation and treats contention
//! as ordinary, which is what its due-work tick already does. So this daemon
//! does the same — it reads without the lock, and takes it briefly to hand
//! arriving events to the same [`Gate`] a local capture goes through. Failing
//! to take it means somebody else is writing, which is not an error and is not
//! reported as one.

mod nudge;

use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

use capture_store::ops::Ops;
use capture_store::paths::StorePaths;
// The round itself. This process decides *when* to run one — on a timer, on
// resume, on a network change — and the round decides what one is.
use capture_store::round::{answer, tick};

/// This process's clock.
///
/// Supplied to the round rather than read inside it: nothing below the surface
/// crates reaches for a clock, so the moment an operation happens is an input
/// to it (DESIGN.md D12).
fn now() -> u64 {
    jiff::Timestamp::now().as_millisecond().max(0) as u64
}
use capture_sync::discovery;
use capture_sync::mdns::Beacon;

/// How often to look around when nothing is happening.
const IDLE: Duration = Duration::from_secs(30);

/// How often to check whether this device has written anything.
///
/// A capture should reach the other device promptly, and thirty seconds is not
/// prompt. §5.4 describes this as a live push to open sessions; what is here
/// instead is a short poll of the local log that runs a round as soon as
/// something is written.
///
/// Polling rather than being told, for the reason `capture_store::watch` gives:
/// a reader that depends on a notification shows stale data for ever if the
/// message is dropped or the writer died between committing and sending it,
/// where a reader that looks for itself is at worst late. The honest
/// difference from §5.4 is latency — one round rather than one socket write —
/// and it is written down rather than claimed away.
const LOOK: Duration = Duration::from_secs(2);

/// What to say when asked, rather than starting.
const USAGE: &str = "\
capture-syncd — find paired devices and reconcile with them

    capture-syncd            run in the foreground until stopped
    capture-syncd --help     this

It takes no other options. Where the store lives comes from $CAPTURE_HOME or the
usual place, and the port from $CAPTURE_SYNC_PORT — which exists for the one case
of two stores on one machine, and is otherwise best left alone.

It reads the store without the writer lock and takes it only for as long as it
takes to hand arriving events over, so it can run alongside the overlay and the
CLI without contending with either.

Sync does nothing until a second device exists. To add one:  capture pair
";

fn main() {
    // Anything but a bare invocation prints this. A daemon that answered an
    // unrecognised flag by opening a listening socket would be one that
    // started when somebody was trying to find out what it does.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        let asked = args.iter().any(|a| a == "--help" || a == "-h");
        print!("{USAGE}");
        std::process::exit(if asked { 0 } else { 2 });
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "capture_syncd=info,capture_sync=info".into()),
        )
        .init();

    if let Err(e) = run() {
        tracing::error!(%e, "the sync daemon stopped");
        std::process::exit(1);
    }
}

fn run() -> Result<(), capture_store::Error> {
    let paths = StorePaths::resolve().ok_or_else(|| {
        capture_store::Error::Usage(
            "cannot work out where the store should live: no home directory, and \
             $CAPTURE_HOME is not set"
                .into(),
        )
    })?;
    let identity = capture_store::store::open(&paths)?;

    tracing::info!(origin = ?identity.dev().short(), "sync daemon starting");

    // Incoming connections get their own thread each. One thread per peer is
    // the shape the transport was written for — `Transport` requires `Send`
    // precisely because of this — and with a handful of devices the count is a
    // handful.
    let port = discovery::sync_port();
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    let (tx, rx) = mpsc::channel();
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = tx.send(Arrival::Dialled(stream));
            }
        });
    }

    // Advertising is best-effort. mDNS is blocked on many corporate and guest
    // networks, and a daemon that refused to run without it would refuse
    // exactly where the static fallback exists to help.
    // The two moments a timer handles worst: a laptop that has just been
    // opened, and a machine that has just joined the network its peers are on.
    {
        let tx = tx.clone();
        nudge::watch_into(tx);
    }

    let mut beacon = match Beacon::new() {
        Ok(b) => Some(b),
        Err(e) => {
            tracing::info!(%e, "no mDNS on this network; falling back to remembered addresses");
            None
        }
    };

    // What this device has written, so a capture does not wait for the idle
    // timer.
    let mut watch = {
        let log = capture_store::log::Log::open_read(&paths.events)?;
        let ops = Ops::open_read(&paths.ops)?;
        capture_store::watch::Watch::start(&log, &ops)?
    };

    let mut round = 0u64;
    loop {
        if let Err(e) = tick(&paths, &identity, &mut beacon, round, port, &now) {
            // A round that fails is a round. The next one may find a different
            // network, a woken peer, or a released lock.
            tracing::warn!(%e, "sync round did not complete");
        }
        round += 1;

        // Answer whoever calls, and go round again as soon as this device
        // writes something — or when the idle timer comes up, whichever is
        // first.
        let deadline = IDLE;
        let mut waited = Duration::ZERO;
        loop {
            match rx.recv_timeout(LOOK) {
                Ok(Arrival::Dialled(stream)) => {
                    if let Err(e) = answer(&paths, &identity, stream, now()) {
                        tracing::info!(%e, "a peer connection ended");
                    }
                }
                Ok(Arrival::Nudged(what)) => {
                    if what == nudge::Nudge::Network {
                        // Everything discovery holds belongs to the old
                        // network: remembered addresses point at it, and the
                        // pseudonyms were derived against it, so peers here
                        // would not be recognised even if they were visible.
                        if let Some(b) = beacon.as_mut() {
                            b.forget();
                        }
                    }
                    tracing::debug!(?what, "syncing now rather than on the timer");
                    break;
                }
                Err(_) => {
                    if wrote_something(&paths, &mut watch) {
                        tracing::debug!("something was captured here; syncing now");
                        break;
                    }
                    waited += LOOK;
                    if waited >= deadline {
                        break;
                    }
                }
            }
        }
    }
}

/// Whether this device has written anything since the last look.
///
/// Errors are treated as "nothing", not as failures: the store may be
/// momentarily locked by the overlay, and a daemon that stopped over that
/// would stop whenever the person was capturing.
fn wrote_something(paths: &StorePaths, watch: &mut capture_store::watch::Watch) -> bool {
    let Ok(log) = capture_store::log::Log::open_read(&paths.events) else {
        return false;
    };
    let Ok(ops) = Ops::open_read(&paths.ops) else {
        return false;
    };
    watch.poll(&log, &ops).map(|c| c.any()).unwrap_or(false)
}

enum Arrival {
    Dialled(std::net::TcpStream),
    /// The machine woke, or the network changed under it.
    Nudged(nudge::Nudge),
}
