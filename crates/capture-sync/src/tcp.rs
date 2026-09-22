//! The [`Transport`] a real connection is.
//!
//! Length-prefixed frames over TCP. Two bytes of length, big-endian, then that
//! many bytes — which caps a frame at 65535, exactly the ceiling a Noise
//! transport message has anyway, so the framing cannot express a message the
//! cipher would refuse.
//!
//! # The one thing this cannot fix
//!
//! [`crate::anti_entropy::exchange`] writes before it reads, and so does the
//! peer. A blocking write into a full socket buffer while the other side is
//! also writing is a deadlock, and the in-memory transport cannot exhibit it
//! because its queues are unbounded.
//!
//! So writes go out on their own thread, with an unbounded queue in front of
//! them. That is not an optimisation: it is what makes the protocol above
//! correct over a real socket, and the reason it is here rather than left as a
//! note in the protocol's documentation.
//!
//! # Why a socket here must have a deadline
//!
//! A round takes the writer lock for its whole length — `round::reconcile` opens
//! a [`Session`](capture_store::session::Session) and holds it across the
//! exchange. A read with no timeout therefore does not merely stall sync: it
//! stalls **capture**, because the next `capture +` waits on the same lock.
//!
//! And an untimed read really can wait for ever. The ordinary case is a phone
//! walking out of Wi-Fi range mid-round: no FIN is ever sent, so the kernel has
//! nothing to report, and with no keepalive `read_exact` blocks indefinitely on
//! a connection to a device that is three streets away. §7.3's rule — an
//! invisible failure is worse than no feature — makes this the worst shape
//! available: the daemon looks healthy, sync looks idle, and captures fail with
//! a lock error nobody can explain.
//!
//! Two bounds, because one is not enough:
//!
//! - **[`IDLE`]**, per read. A peer that has sent nothing for this long is gone.
//! - **[`DEADLINE`]**, per session. Without it a peer sending one byte just
//!   inside every idle window holds the lock for ever while never timing out —
//!   slow-loris by accident, which is what a saturated uplink looks like.
//!
//! Both are generous. Being wrong in the direction of patience costs a delayed
//! round; being wrong the other way costs a capture.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, mpsc};
#[allow(
    clippy::disallowed_types,
    reason = "a socket's own deadline; see the note on `Tcp`"
)]
use std::time::{Duration, Instant};

use crate::transport::{Transport, TransportError};

/// How long a read waits for the peer's next frame before giving up.
///
/// Generous against the slowest thing that legitimately happens between two
/// frames — the peer computing digests over its whole archive — and short enough
/// that a phone that left the network does not hold the writer lock past the
/// user's next capture.
pub const IDLE: Duration = Duration::from_secs(30);

/// The longest a single session may last, however busy it looks.
///
/// This is the bound on how long a round can hold the writer lock. A round that
/// has not finished in five minutes is not going to; the log is append-only and
/// anti-entropy is resumable, so abandoning it loses nothing but the rest of
/// this connection's work.
pub const DEADLINE: Duration = Duration::from_secs(300);

/// How long dialling waits.
///
/// A peer that has gone is the common case — a phone asleep, a laptop shut — and
/// the daemon dials on a timer. Waiting out the kernel's default SYN retries
/// (over two minutes) makes every sweep past a dead peer take longer than the
/// interval between sweeps.
pub const DIAL: Duration = Duration::from_secs(10);

/// How many frames may be queued for writing before a send fails.
///
/// Bounded rather than unbounded, so a peer that has stopped reading costs
/// memory in proportion to this rather than in proportion to the archive. A
/// full queue is a peer that is gone, and failing the send is the honest
/// answer.
const OUTBOX: usize = 1024;

/// One TCP connection, framed.
///
/// # The clock in here, and the boundary it does not cross
///
/// `clippy.toml` refuses `Instant` across the workspace. Its stated subject is
/// `capture-core` — "the fold takes the event set and nothing else" — and the
/// ban is workspace-wide because clippy has one config per invocation, not one
/// per crate. §8.1's boundary is between the fold and everything else, and this
/// is a socket: it is already the least pure thing in the system, it cannot be
/// handed the time by a caller that is blocked inside it, and nothing here
/// reaches a fold.
///
/// Monotonic rather than wall-clock on purpose. A deadline measured against
/// `SystemTime` is undone by an NTP step or a timezone-driven clock jump, both of
/// which are ordinary on a laptop that has just woken up — which is exactly when
/// a round is running.
pub struct Tcp {
    read: TcpStream,
    outbox: SyncSender<Vec<u8>>,
    dead: Arc<AtomicBool>,
    /// When this session must be over, whatever it is doing.
    #[allow(
        clippy::disallowed_types,
        reason = "a socket's own deadline; see the note on `Tcp`"
    )]
    expires: Instant,
}

impl Tcp {
    /// Wrap a connected socket.
    pub fn new(stream: TcpStream) -> std::io::Result<Self> {
        Self::with_limits(stream, IDLE, DEADLINE)
    }

    /// Wrap a socket with limits of your own.
    ///
    /// Exists so the timeouts can be *tested* rather than reasoned about. The
    /// defaults are measured in tens of seconds, and a suite that waited them
    /// out would be a suite nobody runs — which is how an untimed socket stayed
    /// untimed. Shipping code takes [`Self::new`].
    pub fn with_limits(
        stream: TcpStream,
        idle: Duration,
        deadline: Duration,
    ) -> std::io::Result<Self> {
        let read = stream.try_clone()?;
        let mut write = stream;

        // On both halves. The write timeout matters as much as the read one: the
        // writer thread blocking for ever in `write_all` never sets `dead`, so
        // `send` keeps queueing into a socket nothing will ever drain and the
        // peer's absence stays invisible until the outbox fills.
        read.set_read_timeout(Some(idle))?;
        write.set_write_timeout(Some(idle))?;

        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(OUTBOX);
        let dead = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&dead);

        std::thread::spawn(move || {
            for frame in rx {
                if write.write_all(&frame).is_err() || write.flush().is_err() {
                    flag.store(true, Ordering::SeqCst);
                    return;
                }
            }
            // The sender was dropped: this side is finished, and the peer
            // should see the connection close rather than wait on it.
            let _ = write.shutdown(std::net::Shutdown::Write);
        });

        Ok(Self {
            read,
            outbox: tx,
            dead,
            #[allow(
                clippy::disallowed_methods,
                clippy::disallowed_types,
                reason = "a socket's own deadline; see the note on `Tcp`"
            )]
            expires: Instant::now() + deadline,
        })
    }

    /// Dial a peer, giving up on an unanswered connection after [`DIAL`].
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        // Resolved rather than handed straight to `connect_timeout`, which takes
        // a `SocketAddr` and so cannot resolve a name itself. Each candidate
        // gets the full window; a host with both an A and an AAAA record where
        // one is unreachable is the ordinary reason there is more than one.
        let mut last = None;
        for target in std::net::ToSocketAddrs::to_socket_addrs(&addr)? {
            match TcpStream::connect_timeout(&target, DIAL) {
                Ok(stream) => return Self::new(stream),
                Err(e) => last = Some(e),
            }
        }
        Err(last.unwrap_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("no address to dial for {addr}"),
            )
        }))
    }

    /// Whether this session has outrun [`DEADLINE`].
    ///
    /// Checked between frames rather than inside a read, so the true worst case
    /// is `DEADLINE + IDLE` — the deadline passing during a read that then has
    /// to time out on its own. Bounded, which is the property that matters; a
    /// second mechanism to shave the last thirty seconds off would be more
    /// moving parts guarding the same thing.
    #[allow(
        clippy::disallowed_methods,
        clippy::disallowed_types,
        reason = "a socket's own deadline; see the note on `Tcp`"
    )]
    fn expired(&self) -> bool {
        Instant::now() >= self.expires
    }
}

impl Transport for Tcp {
    fn send(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        if self.dead.load(Ordering::SeqCst) {
            return Err(TransportError::Closed);
        }
        let len = u16::try_from(frame.len()).map_err(|_| TransportError::Malformed)?;

        let mut out = Vec::with_capacity(frame.len() + 2);
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(frame);

        match self.outbox.try_send(out) {
            Ok(()) => Ok(()),
            // A full queue is a peer that has stopped reading. Blocking here
            // would reintroduce exactly the deadlock the writer thread exists
            // to prevent.
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                Err(TransportError::Closed)
            }
        }
    }

    fn recv(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        // A socket has no "nothing yet" that does not involve waiting, so this
        // reads. Every caller that uses `recv` on a live connection is asking
        // for the next frame, and the protocol's own loops are blocking.
        self.recv_blocking().map(Some)
    }

    fn recv_blocking(&mut self) -> Result<Vec<u8>, TransportError> {
        if self.expired() {
            return Err(TransportError::TimedOut);
        }
        let mut len = [0u8; 2];
        self.read.read_exact(&mut len).map_err(classify)?;
        let mut frame = vec![0u8; u16::from_be_bytes(len) as usize];
        // The body gets its own idle window rather than sharing the header's.
        // A frame arriving split across packets is normal, and charging the
        // whole message to one timeout would cut off a peer on a slow link
        // mid-frame.
        self.read.read_exact(&mut frame).map_err(classify)?;
        Ok(frame)
    }
}

/// Tell "the peer stopped talking" from "the peer went away".
///
/// A timed-out `read_exact` may have consumed part of a frame, so the stream is
/// unusable either way and both answers end the session. What differs is what is
/// said about it: a timeout is a phone that walked out of range, and reporting
/// that as a failed round would put a fault in front of the user for the most
/// ordinary event in the system.
fn classify(e: std::io::Error) -> TransportError {
    match e.kind() {
        // `WouldBlock` is what a timeout on a blocking socket reports on Linux;
        // `TimedOut` is what it reports elsewhere. Both, because this runs on a
        // phone as well as a desktop.
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => TransportError::TimedOut,
        _ => TransportError::Closed,
    }
}
