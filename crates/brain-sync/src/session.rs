//! Noise_KK: the only way a byte crosses between two devices.
//!
//! # Why KK, and what it assumes
//!
//! Both sides already know each other's static public key — that is what
//! pairing established, and the roster is where it is kept. So there is no
//! certificate, no authority to operate, and nothing to expire: authentication
//! is mutual and falls out of key material the log already carries (§5.2).
//!
//! A device that is not on the roster cannot complete the handshake. Not
//! "is rejected afterwards" — cannot complete it, because it does not hold a
//! key the other side will mix in. That is the property §5.3 wants, and it is
//! the reason this crate has no notion of an unauthenticated peer at all.
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
/// `KK` because both statics are known in advance; X25519 for the
/// Diffie-Hellman, ChaChaPoly for the cipher, BLAKE2s for the hash. Written
/// down here rather than assembled at a call site so two devices cannot end up
/// negotiating with different ideas of what they are speaking.
const PATTERN: &str = "Noise_KK_25519_ChaChaPoly_BLAKE2s";

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

    /// Open a session as the side that answered.
    pub fn accept(
        inner: T,
        local_secret: &[u8; 32],
        peer_public: &[u8; 32],
    ) -> Result<Self, HandshakeError> {
        let state = snow::Builder::new(PATTERN.parse().expect("the pattern is a constant"))
            .local_private_key(local_secret)
            .map_err(|_| HandshakeError::Refused)?
            .remote_public_key(peer_public)
            .map_err(|_| HandshakeError::Refused)?
            .build_responder()
            .map_err(|_| HandshakeError::Refused)?;
        Self::dance(inner, state, false)
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

        Ok(Session {
            noise: state
                .into_transport_mode()
                .map_err(|_| HandshakeError::Refused)?,
            inner,
            sent: 0,
            recvd: 0,
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
