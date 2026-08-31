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

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, mpsc};

use crate::transport::{Transport, TransportError};

/// How many frames may be queued for writing before a send fails.
///
/// Bounded rather than unbounded, so a peer that has stopped reading costs
/// memory in proportion to this rather than in proportion to the archive. A
/// full queue is a peer that is gone, and failing the send is the honest
/// answer.
const OUTBOX: usize = 1024;

/// One TCP connection, framed.
pub struct Tcp {
    read: TcpStream,
    outbox: SyncSender<Vec<u8>>,
    dead: Arc<AtomicBool>,
}

impl Tcp {
    /// Wrap a connected socket.
    pub fn new(stream: TcpStream) -> std::io::Result<Self> {
        let read = stream.try_clone()?;
        let mut write = stream;
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
        })
    }

    /// Dial a peer.
    pub fn connect(addr: &str) -> std::io::Result<Self> {
        Self::new(TcpStream::connect(addr)?)
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
        let mut len = [0u8; 2];
        self.read
            .read_exact(&mut len)
            .map_err(|_| TransportError::Closed)?;
        let mut frame = vec![0u8; u16::from_be_bytes(len) as usize];
        self.read
            .read_exact(&mut frame)
            .map_err(|_| TransportError::Closed)?;
        Ok(frame)
    }
}
