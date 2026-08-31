//! Admitting a device that is not yet on the roster.
//!
//! Everything else in this crate assumes both sides already know each other's
//! static key. This is the one exchange that runs before that is true, and it
//! is therefore the only place where being wrong admits a stranger to the
//! archive rather than merely failing to sync.
//!
//! # Why a short code needs a PAKE
//!
//! The gesture is six digits shown on one device and typed into the other,
//! because the desktop has no camera and §5.3's mutual QR exchange assumes one
//! at both ends.
//!
//! Six digits is a million possibilities. Used directly as a key, anyone who
//! recorded the exchange tries all of them offline in seconds, and the
//! handshake obligingly confirms which guess was right — so the attacker ends
//! up holding the roster secret and a valid admission. SPAKE2 changes what a
//! guess costs: a wrong password yields a different key and no information
//! about the right one, so guessing has to happen online, one attempt at a
//! time, against a device that only allows one.
//!
//! # One attempt
//!
//! A million online guesses is still a million guesses if the code stands
//! still. It does not: the window closes after a single failure, and the next
//! attempt is a fresh code. This is enforced here rather than left to the
//! caller, because "the UI will surely close the window" is the kind of
//! assumption that survives right up until some other caller does not.

use alloc::vec::Vec;

use minicbor::{Decode, Encode};
use spake2::{Ed25519Group, Identity, Password, Spake2};

use crate::session::{HandshakeError, Session};
use crate::transport::Transport;

/// How many digits the user reads across.
///
/// Six is what a person will retype without resenting it. The security does
/// not rest on the number — SPAKE2 and the single attempt do — so it is chosen
/// for the hands rather than for the arithmetic.
pub const CODE_DIGITS: usize = 6;

/// Domain separation, so a transcript from this exchange cannot be replayed
/// into any other protocol that happens to use the same primitive.
const JOINER: &[u8] = b"bsp-pair-joiner";
const ADMITTER: &[u8] = b"bsp-pair-admitter";

/// What went wrong.
#[derive(Debug, PartialEq, Eq)]
pub enum PairingError {
    /// The codes did not match, or something interfered.
    ///
    /// Deliberately one variant rather than several. Distinguishing "wrong
    /// code" from "tampered with" would tell an attacker which of their
    /// guesses was closer, and the user's response is the same either way:
    /// start again with a fresh code.
    Refused,
    /// The exchange completed but the payload was not what it should be.
    Malformed,
    /// The connection went away.
    Disconnected,
}

impl From<HandshakeError> for PairingError {
    fn from(_: HandshakeError) -> Self {
        Self::Refused
    }
}

impl From<crate::transport::TransportError> for PairingError {
    fn from(_: crate::transport::TransportError) -> Self {
        Self::Disconnected
    }
}

impl core::fmt::Display for PairingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            Self::Refused => "pairing refused: the code did not match",
            Self::Malformed => "pairing produced an unreadable reply",
            Self::Disconnected => "the other device went away mid-pairing",
        };
        f.write_str(s)
    }
}

/// Turn eight random bytes into the digits a user reads out.
///
/// Rejection-free and unbiased: the value is reduced modulo the code space
/// from 64 bits, where the bias is smaller than 2^-40 and therefore not the
/// weakest thing here by many orders of magnitude.
pub fn code_from(entropy: [u8; 8]) -> Vec<u8> {
    let mut n = u64::from_be_bytes(entropy) % 10u64.pow(CODE_DIGITS as u32);
    let mut digits = Vec::with_capacity(CODE_DIGITS);
    for _ in 0..CODE_DIGITS {
        digits.push(b'0' + (n % 10) as u8);
        n /= 10;
    }
    digits.reverse();
    digits
}

/// What the joining device says about itself.
///
/// Everything the admitter needs to write a `device_add` and to open a Noise
/// session afterwards. Sent inside the pairing session, never before it.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct Offer {
    /// The signing key membership is keyed by (§5.3).
    #[cbor(n(0), with = "minicbor::bytes")]
    pub pubkey: [u8; 32],
    /// Which incarnation, so the admitter can derive the origin rather than
    /// being told it.
    #[cbor(n(1), with = "minicbor::bytes")]
    pub incarnation: [u8; 16],
    /// The X25519 key a Noise handshake is opened against.
    #[cbor(n(2), with = "minicbor::bytes")]
    pub noise: [u8; 32],
}

/// What the admitting device sends back.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct Grant {
    /// The admitter's own key, so the newcomer has at least one peer it can
    /// dial. Without it the newcomer holds a roster it cannot talk to: it
    /// would be a member unable to complete a handshake with anybody, which
    /// is the deadlock §5.3 describes.
    #[cbor(n(0), with = "minicbor::bytes")]
    pub pubkey: [u8; 32],
    /// The admitter's X25519 key.
    #[cbor(n(1), with = "minicbor::bytes")]
    pub noise: [u8; 32],
    /// Every member key the admitter knows.
    ///
    /// `ByteArray` rather than `[u8; 32]`: a bare array encodes as
    /// thirty-two CBOR integers, which is both five times the bytes and a
    /// different shape from how every other key in this system is written.
    /// The golden vector is what caught it.
    #[n(2)]
    pub members: Vec<minicbor::bytes::ByteArray<32>>,
    /// The secret the mesh advertises under (§5.2).
    #[cbor(n(3), with = "minicbor::bytes")]
    pub secret: Option<[u8; 32]>,
    /// The admitter's incarnation, so the newcomer can admit it back.
    ///
    /// Pairing is a *mutual* exchange (§5.3), and this is what makes the
    /// second half possible: an admission derives the subject's origin from
    /// its key and incarnation rather than being told it, so without the
    /// incarnation the newcomer can hold the admitter's key and still not
    /// vouch for it.
    ///
    /// It matters because two devices that have each run `capture init` have a
    /// root apiece, and only one root confers membership. If the newcomer's
    /// own root happens to win the ordering, the admitter is not a member of
    /// the merged roster and every event it authors is unverifiable there —
    /// permanently, and silently. One-directional pairing left that to a
    /// coin toss.
    #[cbor(n(4), with = "minicbor::bytes")]
    pub incarnation: Option<[u8; 16]>,
}

/// Derive the strong key both sides will authenticate with.
///
/// One side must be A and the other B — SPAKE2's two roles are not
/// interchangeable — and which is which is fixed by function rather than by
/// who displayed the code, so the two devices cannot both pick the same role
/// and silently fail to agree.
fn spake(code: &[u8], joining: bool) -> (Spake2<Ed25519Group>, Vec<u8>) {
    let password = Password::new(code);
    let (a, b) = (Identity::new(JOINER), Identity::new(ADMITTER));
    if joining {
        Spake2::<Ed25519Group>::start_a(&password, &a, &b)
    } else {
        Spake2::<Ed25519Group>::start_b(&password, &a, &b)
    }
}

fn key(shared: Vec<u8>) -> [u8; 32] {
    let mut psk = [0u8; 32];
    psk.copy_from_slice(&blake3::hash(&shared).as_bytes()[..32]);
    psk
}

/// Join a roster: send what this device is, and receive the roster back.
///
/// Returns [`PairingError::Refused`] if the codes did not match. The caller
/// must not retry with the same code — see the module comment.
pub fn join<T: Transport>(
    mut transport: T,
    code: &[u8],
    offer: &Offer,
) -> Result<Grant, PairingError> {
    let (state, mine) = spake(code, true);
    transport.send(&mine)?;
    let theirs = transport.recv_blocking()?;
    let shared = state.finish(&theirs).map_err(|_| PairingError::Refused)?;

    // The Noise handshake is what confirms the key. SPAKE2 alone leaves both
    // sides holding *a* key with no evidence it is the same one, and acting on
    // an unconfirmed key is how a wrong guess gets treated as a right one.
    let mut session = Session::initiate_paired(transport, &key(shared))?;

    session.send(&encode(offer))?;
    let reply = session.recv_blocking()?;
    minicbor::decode(&reply).map_err(|_| PairingError::Malformed)
}

/// Admit a device: receive what it is, and send the roster.
///
/// The grant is produced by a callback rather than passed in, so the caller
/// writes the `device_add` at the moment the offer is known to be authentic
/// and not a moment earlier.
pub fn admit<T: Transport, F>(
    mut transport: T,
    code: &[u8],
    grant: F,
) -> Result<Offer, PairingError>
where
    F: FnOnce(&Offer) -> Grant,
{
    let (state, mine) = spake(code, false);
    let theirs = transport.recv_blocking()?;
    transport.send(&mine)?;
    let shared = state.finish(&theirs).map_err(|_| PairingError::Refused)?;

    let mut session = Session::accept_paired(transport, &key(shared))?;

    let offered = session.recv_blocking()?;
    let offer: Offer = minicbor::decode(&offered).map_err(|_| PairingError::Malformed)?;
    session.send(&encode(&grant(&offer)))?;
    Ok(offer)
}

fn encode<T: minicbor::Encode<()>>(v: &T) -> Vec<u8> {
    let mut b = Vec::new();
    minicbor::encode(v, &mut b).expect("encoding into Vec is infallible");
    b
}
