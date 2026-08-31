//! Noise_IK: the only way a byte crosses between two devices.
//!
//! # Why IK, and not the KK the design first named
//!
//! Both patterns give mutual authentication from key material the log already
//! carries — no certificate, no authority to operate, nothing to expire
//! (§5.2). The difference is *when* each side learns who the other is.
//!
//! KK requires both statics to be known before the handshake starts. That is
//! true of the side that dials — it picked who to call — and false of the side
//! that answers, which has a TCP connection and no idea who opened it. It
//! cannot ask, because §5.2 forbids any message outside an established
//! session, and it cannot try each member in turn, because a failed attempt
//! consumes the connection. KK is simply not implementable for an inbound
//! connection in this topology, and that was discovered by writing the daemon
//! rather than by reading the design.
//!
//! IK keeps what mattered and moves what did not. The initiator must still
//! know the responder's static key, and that key lives in the log — so a
//! device without the archive cannot produce a valid first message and cannot
//! complete the handshake. The responder learns the initiator's key as part of
//! the handshake and checks it against the roster.
//!
//! What this costs, stated plainly: a **retired** device still holds the log,
//! so it can complete the handshake and is refused on the roster check
//! afterwards rather than being unable to start. §5.3 already requires that
//! check — "drops existing sessions with that origin immediately and refuses
//! new ones" — so the mechanism exists; under IK it is load-bearing rather
//! than belt-and-braces. A device that was never a member is still unable to
//! begin.
//!
//! # The rule this module makes unbreakable
//!
//! Everything on the wire is inside the session. There is no pre-handshake
//! message, no plaintext fallback, and no way to express either: a [`Session`]
//! is the only thing that can carry a [`crate::Message`], and it does not
//! exist until the handshake has finished. A failed handshake yields no
//! session, so the connection has nothing to say and is closed.
//!
//! §5.2 states that as an absolute because it is violated by convenience — the
//! blob plane moves large byte ranges and "just this one over a plain socket"
//! is exactly the accident it forbids.

use alloc::vec;
use alloc::vec::Vec;

use crate::transport::{Transport, TransportError};

/// The Noise pattern, named once.
///
/// `IK`: the initiator knows the responder's static in advance and transmits
/// its own; X25519 for the Diffie-Hellman, ChaChaPoly for the cipher, BLAKE2s
/// for the hash. Written down here rather than assembled at a call site so two
/// devices cannot end up negotiating with different ideas of what they are
/// speaking.
const PATTERN: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";

/// The pattern used for pairing, before either side is on the other's roster.
///
/// `NNpsk0`: neither device knows the other's static key yet — establishing
/// that is what pairing is for — so authentication comes entirely from a
/// pre-shared key mixed in before anything else.
///
/// The psk is not the six-digit code. It is the strong key SPAKE2 derives from
/// it (see [`crate::pairing`]), because a psk *is* the authentication here: a
/// six-digit one would be a million offline guesses for anyone who recorded
/// the exchange, and the handshake would confirm the right guess for them.
const PAIRING_PATTERN: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";

/// The largest payload a single Noise message can carry.
///
/// A Noise transport message caps at 65535 bytes including the 16-byte
/// authentication tag, so this is what is left for content. Blob chunks are
/// 32 KiB precisely so one fits inside a message with room for framing — an
/// earlier draft of the design specified 65536-byte chunks, which do not fit at
/// all, and the number is pinned by a test rather than remembered.
pub const MAX_PAYLOAD: usize = 65519;

/// How many messages a session sends before rekeying.
///
/// Noise's nonce is 64 bits and would take longer than the universe to exhaust
/// honestly, so this is not about running out. It is forward secrecy within a
/// long-lived link: a key recovered from a device tomorrow should not decrypt
/// what it sent this morning, and a session that syncs an archive can be open
/// for hours.
pub const REKEY_AFTER: u64 = 10_000;

/// Why a handshake did not produce a session.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum HandshakeError {
    /// The peer could not prove it holds the key we expect.
    ///
    /// Deliberately one variant covering every cryptographic failure. A peer
    /// learns that it failed and nothing else — which of the checks it failed
    /// is information it has not earned.
    Refused,
    /// The channel closed before the handshake finished.
    Interrupted,
}

impl From<TransportError> for HandshakeError {
    fn from(_: TransportError) -> Self {
        HandshakeError::Interrupted
    }
}

/// An authenticated, encrypted channel to one peer.
///
/// The only thing in this crate that can carry a message, and it cannot be
/// constructed except by a completed handshake.
pub struct Session<T: Transport> {
    noise: snow::TransportState,
    inner: T,
    sent: u64,
    recvd: u64,
    /// Who is at the other end, as the handshake established it.
    peer: Option<[u8; 32]>,
}

impl<T: Transport> Session<T> {
    /// Open a session as the side that dialled.
    pub fn initiate(
        inner: T,
        local_secret: &[u8; 32],
        peer_public: &[u8; 32],
    ) -> Result<Self, HandshakeError> {
        let state = snow::Builder::new(PATTERN.parse().expect("the pattern is a constant"))
            .local_private_key(local_secret)
            .map_err(|_| HandshakeError::Refused)?
            .remote_public_key(peer_public)
            .map_err(|_| HandshakeError::Refused)?
            .build_initiator()
            .map_err(|_| HandshakeError::Refused)?;
        Self::dance(inner, state, true)
    }

    /// Open a session as the side that answered, learning who called.
    ///
    /// Takes no peer key, because the whole point is that this side does not
    /// have one yet. [`Session::peer`] carries what the handshake established,
    /// and the caller must check it against the roster: a retired device still
    /// holds the log and can therefore still complete this.
    pub fn accept(inner: T, local_secret: &[u8; 32]) -> Result<Self, HandshakeError> {
        let state = snow::Builder::new(PATTERN.parse().expect("the pattern is a constant"))
            .local_private_key(local_secret)
            .map_err(|_| HandshakeError::Refused)?
            .build_responder()
            .map_err(|_| HandshakeError::Refused)?;
        Self::dance(inner, state, false)
    }

    /// The static key of the device at the other end.
    ///
    /// Established by the handshake rather than claimed by the peer. `None`
    /// only for a pairing session, where neither side has a static key yet —
    /// which is what pairing exists to fix.
    pub fn peer(&self) -> Option<[u8; 32]> {
        self.peer
    }

    /// Open a pairing session as the side that dialled.
    ///
    /// No static keys: the point of the exchange is to learn them. What
    /// authenticates it is the psk, and a handshake that completes is proof
    /// both sides derived the same one — which is proof the code was entered
    /// correctly, since nothing else could have produced it.
    pub fn initiate_paired(inner: T, psk: &[u8; 32]) -> Result<Self, HandshakeError> {
        let state = snow::Builder::new(PAIRING_PATTERN.parse().expect("the pattern is a constant"))
            .psk(0, psk)
            .map_err(|_| HandshakeError::Refused)?
            .build_initiator()
            .map_err(|_| HandshakeError::Refused)?;
        Self::dance(inner, state, true)
    }

    /// Open a pairing session as the side that answered.
    pub fn accept_paired(inner: T, psk: &[u8; 32]) -> Result<Self, HandshakeError> {
        let state = snow::Builder::new(PAIRING_PATTERN.parse().expect("the pattern is a constant"))
            .psk(0, psk)
            .map_err(|_| HandshakeError::Refused)?
            .build_responder()
            .map_err(|_| HandshakeError::Refused)?;
        Self::dance(inner, state, false)
    }

    /// Run the handshake to completion, or fail without a session.
    ///
    /// Blocking, and deliberately so: there is nothing useful to do on this
    /// connection until it finishes, and the caller is a thread that exists to
    /// serve one peer.
    fn dance(
        mut inner: T,
        mut state: snow::HandshakeState,
        initiator: bool,
    ) -> Result<Self, HandshakeError> {
        let mut buf = vec![0u8; u16::MAX as usize];

        // The initiator speaks first; after that they alternate until the
        // pattern is done. `is_handshake_finished` is the pattern's own answer
        // rather than a message count, so KK's three messages are not written
        // down twice.
        let mut my_turn = initiator;
        while !state.is_handshake_finished() {
            if my_turn {
                let n = state
                    .write_message(&[], &mut buf)
                    .map_err(|_| HandshakeError::Refused)?;
                inner.send(&buf[..n])?;
            } else {
                let msg = inner.recv_blocking()?;
                state
                    .read_message(&msg, &mut buf)
                    .map_err(|_| HandshakeError::Refused)?;
            }
            my_turn = !my_turn;
        }

        let peer = state
            .get_remote_static()
            .and_then(|k| <[u8; 32]>::try_from(k).ok());

        Ok(Session {
            noise: state
                .into_transport_mode()
                .map_err(|_| HandshakeError::Refused)?,
            inner,
            sent: 0,
            recvd: 0,
            peer,
        })
    }

    /// Encrypt and send one payload.
    pub fn send(&mut self, payload: &[u8]) -> Result<(), TransportError> {
        if payload.len() > MAX_PAYLOAD {
            return Err(TransportError::Malformed);
        }
        let mut out = vec![0u8; payload.len() + 16];
        let n = self
            .noise
            .write_message(payload, &mut out)
            .map_err(|_| TransportError::Malformed)?;
        self.inner.send(&out[..n])?;

        self.sent += 1;
        if self.sent.is_multiple_of(REKEY_AFTER) {
            // Only outgoing. Each direction is rotated by the count of
            // messages in that direction, so the sender's outgoing and the
            // receiver's incoming move together even when the traffic is
            // entirely one-way — which, for a device catching up after a week
            // off, it is.
            self.noise.rekey_outgoing();
        }
        Ok(())
    }

    /// Receive and decrypt one payload, if the peer has sent one.
    pub fn recv(&mut self) -> Result<Option<Vec<u8>>, TransportError> {
        let Some(frame) = self.inner.recv()? else {
            return Ok(None);
        };
        self.open(&frame).map(Some)
    }

    /// Wait for one payload.
    ///
    /// For the exchanges that are a conversation rather than a stream —
    /// pairing, and the handshake — where there is nothing else this thread
    /// could usefully do until the peer answers.
    pub fn recv_blocking(&mut self) -> Result<Vec<u8>, TransportError> {
        let frame = self.inner.recv_blocking()?;
        self.open(&frame)
    }

    fn open(&mut self, frame: &[u8]) -> Result<Vec<u8>, TransportError> {
        let mut out = vec![0u8; frame.len()];
        let n = self
            .noise
            .read_message(frame, &mut out)
            .map_err(|_| TransportError::Malformed)?;
        out.truncate(n);

        self.recvd += 1;
        if self.recvd.is_multiple_of(REKEY_AFTER) {
            self.noise.rekey_incoming();
        }
        Ok(out)
    }
}
