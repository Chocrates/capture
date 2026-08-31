//! The one place in the system that generates randomness.
//!
//! `capture-core` cannot do this — it is `no_std`, so the syscall is unreachable,
//! and a lint bans the call besides. That is deliberate: the fold must depend on
//! the events and nothing else, and randomness is the clearest example of
//! something that would break it.
//!
//! But the values still have to come from somewhere. This module is that
//! somewhere: the single point where entropy enters, so the boundary is one
//! function rather than a habit spread across call sites.

use capture_core::{Incarnation, Nonce};

use crate::{Error, Result};

/// Fill a buffer with cryptographically secure random bytes.
///
/// The lint that forbids this elsewhere is silenced here and only here, because
/// this crate is the boundary the lint exists to define.
#[allow(
    clippy::disallowed_methods,
    reason = "capture-store is the boundary; entropy enters the system here and nowhere else"
)]
pub fn fill(buf: &mut [u8]) -> Result<()> {
    getrandom::fill(buf)
        .map_err(|e| Error::Io(std::io::Error::other(format!("entropy unavailable: {e}"))))
}

/// A fresh nonce for an event payload.
///
/// Every payload carries one so that the surviving fingerprint of a redacted
/// payload cannot be checked against a guess.
pub fn nonce() -> Result<Nonce> {
    let mut b = [0u8; 16];
    fill(&mut b)?;
    Ok(Nonce(b))
}

/// A fresh incarnation, distinguishing one installation of a key from the next.
///
/// Random rather than sequential: a counter would have to be read from the very
/// artefact a restore has just lost, so two restores would mint the same origin.
pub fn incarnation() -> Result<Incarnation> {
    let mut b = [0u8; 16];
    fill(&mut b)?;
    Ok(Incarnation(b))
}

/// A fresh secret for the mesh to advertise under (§5.2).
///
/// Thirty-two bytes because it keys a MAC, not because anything about the
/// advertised values needs that much: the pseudonym and digest are eight bytes
/// each. What needs the length is the key, since holding it is what
/// membership means to the discovery layer.
pub fn secret() -> Result<[u8; 32]> {
    let mut b = [0u8; 32];
    fill(&mut b)?;
    Ok(b)
}

/// Eight bytes, for a pairing code.
///
/// The code a person reads across is derived from these rather than chosen, so
/// it is unguessable in the way the exchange assumes — SPAKE2 makes each wrong
/// guess cost an online attempt, and that is only worth anything if the right
/// answer cannot be predicted.
pub fn pairing_entropy() -> Result<[u8; 8]> {
    let mut b = [0u8; 8];
    fill(&mut b)?;
    Ok(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonces_differ() {
        assert_ne!(nonce().unwrap().0, nonce().unwrap().0);
    }

    #[test]
    fn incarnations_differ() {
        // The property a counter would not have.
        assert_ne!(incarnation().unwrap().0, incarnation().unwrap().0);
    }

    #[test]
    fn output_is_not_all_zero() {
        let mut b = [0u8; 32];
        fill(&mut b).unwrap();
        assert!(b.iter().any(|&x| x != 0));
    }
}
