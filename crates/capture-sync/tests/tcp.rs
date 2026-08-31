//! The transport over a real socket.
//!
//! Loopback rather than a mock, because what is being tested is the part the
//! in-memory transport cannot model: a finite send buffer.

use std::net::{TcpListener, TcpStream};

use capture_sync::anti_entropy::{Archive, exchange};
use capture_sync::message::VersionVector;
use capture_sync::session::Session;
use capture_sync::tcp::Tcp;
use capture_sync::transport::{Transport, TransportError};

fn connected() -> (Tcp, Tcp) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().unwrap();
    let accepting = std::thread::spawn(move || listener.accept().unwrap().0);
    let client = TcpStream::connect(addr).expect("connect");
    let server = accepting.join().unwrap();
    (Tcp::new(client).unwrap(), Tcp::new(server).unwrap())
}

#[test]
fn a_frame_crosses_intact() {
    let (mut a, mut b) = connected();
    a.send(b"the quick brown fox").unwrap();
    assert_eq!(b.recv_blocking().unwrap(), b"the quick brown fox");
}

#[test]
fn frames_keep_their_boundaries() {
    // TCP is a stream: without length prefixes two sends arrive as one read,
    // and a Noise frame split across two reads decrypts as neither.
    let (mut a, mut b) = connected();
    a.send(b"one").unwrap();
    a.send(b"two").unwrap();
    a.send(&[0u8; 40_000]).unwrap();

    assert_eq!(b.recv_blocking().unwrap(), b"one");
    assert_eq!(b.recv_blocking().unwrap(), b"two");
    assert_eq!(b.recv_blocking().unwrap().len(), 40_000);
}

#[test]
fn a_frame_too_large_to_frame_is_refused() {
    // The length prefix is two bytes, which is exactly the ceiling a Noise
    // transport message has — so the framing cannot express a message the
    // cipher would refuse.
    let (mut a, _b) = connected();
    assert_eq!(a.send(&[0u8; 70_000]), Err(TransportError::Malformed));
}

#[test]
fn a_closed_connection_is_reported_rather_than_waited_on() {
    let (mut a, b) = connected();
    drop(b);
    // The first send may still succeed into the local buffer; a read cannot.
    let _ = a.send(b"anything");
    assert!(a.recv_blocking().is_err());
}

/// An archive holding nothing, to drive an exchange over a socket.
#[derive(Default)]
struct Empty;

impl Archive for Empty {
    fn heads(&self) -> VersionVector {
        VersionVector::new()
    }
    fn range(&self, _dev: &str, _from: u64, _to: u64) -> Vec<capture_core::envelope::Envelope> {
        Vec::new()
    }
    fn ingest(&mut self, _e: capture_core::envelope::Envelope) -> bool {
        true
    }
}

#[test]
fn a_full_exchange_completes_over_a_socket() {
    // Both sides write before either reads. Over a socket with a finite send
    // buffer that is a deadlock unless writes are queued off-thread, which is
    // the requirement the protocol's documentation states and this transport
    // owes it.
    let (a, b) = connected();

    let responder = std::thread::spawn(move || {
        let mut s = Session::accept_paired(b, &[9; 32]).expect("session");
        exchange(&mut s, &mut Empty).expect("the responder finished")
    });
    let mut s = Session::initiate_paired(a, &[9; 32]).expect("session");
    let mine = exchange(&mut s, &mut Empty).expect("the initiator finished");

    let theirs = responder.join().expect("no panic");
    assert_eq!(mine.received, 0);
    assert_eq!(theirs.received, 0);
}
