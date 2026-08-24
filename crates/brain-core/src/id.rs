//! Identities: origins, events, and body commitments.
//!
//! Every type here is 32 bytes of BLAKE3 and is **frozen** — these values are
//! written into the log and hashed into each other, so the derivations cannot
//! change without forking the archive.

use minicbor::{Decode, Encode};

use crate::Incarnation;

/// Origin identity: `dev = blake3(pubkey ‖ incarnation)`.
///
/// Not a device *name*. A device that is restored, reinstalled, or has its
/// database replaced mints a fresh [`Incarnation`] and therefore becomes a new
/// origin — while remaining the same roster member, because membership is keyed
/// by public key rather than by origin.
///
/// That indirection is what stops a restored replica reusing sequence positions
/// its peers already hold, which is silent bidirectional data loss.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Encode, Decode)]
#[cbor(transparent)]
pub struct Dev(#[cbor(n(0), with = "minicbor::bytes")] pub [u8; 32]);

impl Dev {
    /// Derive an origin from a device public key and an incarnation.
    pub fn derive(pubkey: &[u8; 32], incarnation: &Incarnation) -> Self {
        let mut h = blake3::Hasher::new();
        h.update(pubkey);
        h.update(&incarnation.0);
        Dev(*h.finalize().as_bytes())
    }

    /// Short form for display. Never used for identity.
    pub fn short(&self) -> [u8; 4] {
        [self.0[0], self.0[1], self.0[2], self.0[3]]
    }
}

/// Event identity: `blake3(canonical header)`, where the header carries a
/// [`BodyHash`] rather than the body itself.
///
/// The indirection is deliberate. Committing to the body directly would mean a
/// redacted event's id could never be recomputed, so integrity checking would
/// break permanently on exactly the rows most in need of an audit trail. With a
/// body *hash* in the header, a redacted event still recomputes and still
/// verifies.
///
/// The consequence — and it is easy to miss — is that `id` and `sig`
/// authenticate the body's hash, **not the body**. So `blake3(body) == bhash`
/// must be checked separately on ingest, or a relaying device can substitute
/// content that still verifies.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Encode, Decode)]
#[cbor(transparent)]
pub struct EventId(#[cbor(n(0), with = "minicbor::bytes")] pub [u8; 32]);

/// Commitment to a canonical event body: `blake3(canonical_body)`.
///
/// Survives redaction, which is the whole point — and is also why every body
/// carries a 16-byte nonce at field 0. Without one this would be an *unsalted*
/// commitment to content redaction was supposed to destroy, living in every
/// replica and every export forever. The bodies redaction exists for are short
/// and low-entropy, so an adversary holding an old export could confirm a guess
/// by hashing candidates until one matched.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Encode, Decode)]
#[cbor(transparent)]
pub struct BodyHash(#[cbor(n(0), with = "minicbor::bytes")] pub [u8; 32]);

impl BodyHash {
    /// Commit to already-canonical body bytes.
    pub fn of(canonical_body: &[u8]) -> Self {
        BodyHash(*blake3::hash(canonical_body).as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_changes_with_incarnation() {
        let pk = [1u8; 32];
        let a = Dev::derive(&pk, &Incarnation([0u8; 16]));
        let b = Dev::derive(&pk, &Incarnation([1u8; 16]));
        assert_ne!(a, b, "a restored device must be a distinct origin");
    }

    #[test]
    fn origin_is_stable_for_the_same_inputs() {
        let pk = [7u8; 32];
        let inc = Incarnation([9u8; 16]);
        assert_eq!(Dev::derive(&pk, &inc), Dev::derive(&pk, &inc));
    }

    #[test]
    fn different_keys_give_different_origins() {
        let inc = Incarnation([9u8; 16]);
        assert_ne!(Dev::derive(&[1u8; 32], &inc), Dev::derive(&[2u8; 32], &inc));
    }

    #[test]
    fn body_hash_covers_every_byte() {
        let a = BodyHash::of(b"hello");
        let b = BodyHash::of(b"hellp");
        assert_ne!(a, b);
    }
}
