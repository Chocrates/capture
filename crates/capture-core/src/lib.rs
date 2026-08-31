//! `capture-core` — the part of the system that can be wrong forever.
//!
//! Event identity, the canonical encoding, the fold, and the conflict rules live
//! here. Nothing else does.
//!
//! # The purity boundary
//!
//! This crate is `no_std` on purpose, and not for portability. It is the
//! enforcement mechanism for a rule that is otherwise only a good intention:
//!
//! > **The fold takes the event set and nothing else.** Not the wall clock, not
//! > the roster as it currently stands, not the filesystem.
//!
//! Two earlier drafts of the design broke that rule in ways that looked like
//! fixes — quarantining future-dated events out of the projection (which made
//! the same log render differently on three machines) and clamping incoming
//! timestamps (which destroyed causality). Both were caught by review rather
//! than by a test, because a test cannot easily observe "this function could
//! have taken a clock".
//!
//! Under `no_std`, `std::time::SystemTime`, `std::time::Instant` and `std::fs`
//! are not nameable, so the mistake cannot compile. Entropy, clocks and
//! timezone data are passed **in** as values at the boundary — see
//! [`Nonce`], [`Incarnation`], and the HLC types when they land in group 2.
//!
//! `clippy.toml` and `deny.toml` catch what the type system does not.

#![no_std]
#![forbid(unsafe_code)]
#![deny(clippy::disallowed_methods, clippy::disallowed_types)]
#![warn(missing_docs)]

extern crate alloc;

/// How long a redaction waits before it destroys anything.
///
/// The whole point of the delay: redaction is the only irreversible operation
/// in the system, and the most likely moment to fire one at the wrong note is
/// the moment you are certain. A window costs nothing when the redaction was
/// intended and is the only recovery when it was not.
///
/// Measured from the redaction's own timestamp, so every replica destroys at
/// the same instant rather than each starting its own clock on arrival.
pub const REDACT_WINDOW_MS: u64 = 30 * 60 * 1000;

pub mod body;
pub mod envelope;
pub mod event;
pub mod fold;
pub mod hlc;
pub mod id;
pub mod link;
pub mod roster;
pub mod slug;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use envelope::{Envelope, Rejection, Verdict};
pub use event::{ENVELOPE_VERSION, Header, Kind};
pub use fold::{Note, Projection, fold};
pub use hlc::{Hlc, HlcState};
pub use id::{BodyHash, Dev, EventId};
pub use slug::{SLUG_VERSION, Slug, SlugBasis};

/// A 16-byte random value that prefixes every canonical event body.
///
/// This is a security requirement, not a formality. `bhash` is a commitment to
/// the body, it survives redaction, and it lives in every replica and every
/// export forever. Without a nonce it is an *unsalted* commitment to content
/// that redaction was supposed to destroy — and the content redaction exists
/// for (a pasted credential, someone else's personal details) is short and
/// low-entropy, so anyone holding an old export could confirm a guess by
/// hashing candidates until one matched.
///
/// With the nonce, the commitment is hiding. The nonce dies with the body.
///
/// `capture-core` cannot generate one — that needs entropy, which needs a syscall.
/// The caller supplies it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Nonce(pub [u8; 16]);

/// Distinguishes one installation of a device key from the next.
///
/// Origin identity is `dev = H(pubkey ‖ incarnation)`, and the incarnation is
/// minted fresh on every restore, reinstall, or database replacement.
///
/// It is **random, not a counter**. A counter has to be read from somewhere, and
/// the thing you would read it from is exactly what was just lost: restoring
/// twice, or restoring from a stale export, would mint the same origin twice and
/// collide at every sequence position. An incarnation is a nonce, not a version
/// — there is nothing to increment from and nothing a stale copy can reproduce.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Incarnation(pub [u8; 16]);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonce_and_incarnation_are_plain_values() {
        // Guards the boundary: these are carried in, never produced here.
        // If this crate ever gains the ability to generate one, the no_std
        // build breaks first — which is the point.
        let n = Nonce([7u8; 16]);
        let i = Incarnation([9u8; 16]);
        assert_eq!(n.0.len(), 16);
        assert_eq!(i.0.len(), 16);
    }
}
