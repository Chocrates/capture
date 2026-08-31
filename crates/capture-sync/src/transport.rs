//! What a message travels over, stated as a trait so it need not be a socket.
//!
//! # Every protocol test runs without a network
//!
//! This is the point of the trait, and it is worth more than the abstraction.
//! A suite that opens sockets is slow, is flaky on a laptop that changes
//! networks, and fails for reasons that have nothing to do with the protocol —
//! so it gets marked ignored, and then it is not a suite. The protocol logic
//! here talks to [`Transport`]; the tests hand it [`Pair`], which is two queues
//! and no operating system.
//!
//! What the trait deliberately does not model is the handshake. A `Transport`
//! is a channel that already exists and is already authenticated: by the time
//! anything here has one, Noise_KK has completed and the peer is on the roster
//! (§5.2). Nothing in this crate can send a byte before that, because there is
//! no way to express it.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Why a message could not be sent or received.
///
/// Deliberately small. The protocol's answer to almost every failure is the
/// same — the session is over, anti-entropy will try again later — and a
/// taxonomy of socket errors would invite handling them differently, which is
/// how a retry loop that should not exist gets written.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TransportError {
    /// The peer is gone, or the channel was closed under us.
    Closed,
    /// A frame arrived that this build cannot read.
    Malformed,
}

/// An authenticated, ordered, message-oriented channel to one peer.
///
/// Ordered and reliable within a session, because that is what TCP inside a
/// Noise session gives. Sessions themselves are unreliable: one can end at any
/// point, and the protocol above must never depend on having finished
/// anything.
/// `Send`, because the daemon is one thread per peer session (§ the M3 design's
/// I/O decision). A transport that could not cross a thread boundary would be
/// discovered at the point the daemon is written rather than here.
pub trait Transport: Send {
    /// Hand one message to the peer.
    fn send(&mut self, frame: &[u8]) -> Result<(), TransportError>;

    /// Take the next message, or `None` if none is waiting.
    ///
    /// Non-blocking, so a caller can serve both directions without threads and
    /// a test can drive both ends by hand.
    fn recv(&mut self) -> Result<Option<Vec<u8>>, TransportError>;

    /// Wait for the next message.
    ///
    /// The handshake needs this and the protocol loop does not: there is
    /// nothing useful to do on a connection whose handshake is half finished,
    /// where the loop above a live session has both directions to serve.
    ///
    /// The default spins with a yield, which is what an in-memory transport
    /// can do honestly. A socket overrides it with a blocking read.
    fn recv_blocking(&mut self) -> Result<Vec<u8>, TransportError> {
        loop {
            match self.recv()? {
                Some(m) => return Ok(m),
                None => std::thread::yield_now(),
            }
        }
    }
}

/// Two ends of an in-memory channel, for tests.
///
/// Not a mock: it is a real implementation of the same contract, and the
/// protocol cannot tell it from a socket. What it removes is the operating
/// system, which is the part that makes a test slow and unreliable rather than
/// the part that makes it meaningful.
///
/// The two ends share their queues rather than borrowing them, because they
/// are held at once and each one's inbox is the other's outbox — which no
/// arrangement of `&mut` can express.
#[derive(Default)]
pub struct Pair {
    a: Queue,
    b: Queue,
    closed: Arc<AtomicBool>,
    wire: Wire,
}

/// A copy of every frame that crossed, in order.
///
/// Recording rather than intercepting: an exchange of several messages cannot
/// be inspected by stealing frames off the queue, because the frames stolen
/// are the ones the other side then never receives. This observes without
/// participating, which is exactly the position a passive attacker is in.
type Wire = Arc<Mutex<Vec<u8>>>;

/// Shared rather than borrowed, and shareable *across threads*, because each
/// end's inbox is the other's outbox and both are held at once — by two
/// threads, in the shape the daemon actually has.
type Queue = Arc<Mutex<VecDeque<Vec<u8>>>>;

impl Pair {
    pub fn new() -> Pair {
        Pair::default()
    }

    /// The channel is over, in both directions.
    ///
    /// A session ending mid-exchange is ordinary rather than exceptional — a
    /// laptop closing, a network changing — so the tests have to be able to
    /// cause one, including part-way through a conversation.
    pub fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }

    /// The two ends. Each writes to the other's queue and reads its own.
    pub fn ends(&self) -> (End, End) {
        (
            End {
                inbox: Arc::clone(&self.a),
                outbox: Arc::clone(&self.b),
                closed: Arc::clone(&self.closed),
                wire: Arc::clone(&self.wire),
            },
            End {
                inbox: Arc::clone(&self.b),
                outbox: Arc::clone(&self.a),
                closed: Arc::clone(&self.closed),
                wire: Arc::clone(&self.wire),
            },
        )
    }

    /// Everything that has crossed, as a passive observer would have it.
    pub fn wire(&self) -> Vec<u8> {
        self.wire.lock().map(|w| w.clone()).unwrap_or_default()
    }
}

/// One side of a [`Pair`].
pub struct End {
    inbox: Queue,
    outbox: Queue,
    closed: Arc<AtomicBool>,
    wire: Wire,
}

impl Transport for End {
    fn send(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(TransportError::Closed);
        }
        if let Ok(mut wire) = self.wire.lock() {
            wire.extend_from_slice(frame);
        }
        self.outbox
            .lock()
            .map_err(|_| TransportError::Closed)?
            .push_back(frame.to_vec());
        Ok(())
    }

    fn recv(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(TransportError::Closed);
        }
        Ok(self
            .inbox
            .lock()
            .map_err(|_| TransportError::Closed)?
            .pop_front())
    }
}

impl Drop for End {
    /// One end going away ends the channel.
    ///
    /// This is what a peer disconnecting is, and modelling it matters more than
    /// it looks: a handshake that fails on one side leaves the other waiting
    /// for a reply that is never coming, and without this it waits for ever.
    /// The first version of the refusal tests hung exactly there.
    ///
    /// A successful session keeps its end alive inside the `Session`, so this
    /// fires when a side has genuinely finished with the connection.
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_one_end_sends_the_other_receives() {
        let pair = Pair::new();
        {
            let (mut a, mut b) = pair.ends();
            a.send(b"first").unwrap();
            a.send(b"second").unwrap();
            // In order, because a session is ordered even though sessions are
            // not reliable.
            assert_eq!(b.recv().unwrap().as_deref(), Some(&b"first"[..]));
            assert_eq!(b.recv().unwrap().as_deref(), Some(&b"second"[..]));
            assert_eq!(
                b.recv().unwrap(),
                None,
                "nothing was sent, so nothing waits"
            );
        }
    }

    #[test]
    fn each_end_reads_only_what_the_other_wrote() {
        // The mistake this catches is one shared queue, where a side receives
        // its own message and a protocol test passes without a peer.
        let pair = Pair::new();
        let (mut a, mut b) = pair.ends();
        a.send(b"mine").unwrap();
        assert_eq!(a.recv().unwrap(), None, "an end received its own message");
        assert_eq!(b.recv().unwrap().as_deref(), Some(&b"mine"[..]));
    }

    #[test]
    fn a_closed_session_refuses_both_directions() {
        let pair = Pair::new();
        pair.close();
        let (mut a, mut b) = pair.ends();
        assert_eq!(a.send(b"x"), Err(TransportError::Closed));
        assert_eq!(b.recv(), Err(TransportError::Closed));
    }
}
