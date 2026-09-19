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

/// How much of a signing key a person is shown.
///
/// Six bytes — twelve hex characters — and the number is here rather than in
/// each surface because it is the thing somebody compares when deciding
/// whether a device in the roster is the one they lost. Two screens showing
/// different amounts of the same key is a disagreement at exactly the moment
/// somebody is trying to tell devices apart.
///
/// It was written out seven times before this: in `round.rs`, twice in the
/// command line, once in the desktop window, and three times in the FFI.
pub const SHOWN_KEY_BYTES: usize = 6;

/// A signing key as the surfaces show it.
///
/// A prefix nobody reads is worse than one short enough that they do, which is
/// the whole argument for showing part of a key rather than all of it.
pub fn short_key(key: &[u8]) -> alloc::string::String {
    hexed(&key[..SHOWN_KEY_BYTES.min(key.len())])
}

/// Bytes as lowercase hex.
pub fn hexed(bytes: &[u8]) -> alloc::string::String {
    use alloc::string::String;
    bytes.iter().fold(String::new(), |mut out, b| {
        use core::fmt::Write;
        // Infallible into a String, and the alternative — `format!` per byte
        // and collect — allocates once per byte.
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// Lowercase hex back to bytes, or nothing.
///
/// **An empty string is not zero bytes, it is not hex.** The two copies this
/// replaces disagreed on exactly that: one returned `None`, the other returned
/// an empty vector. They sat either side of §5.3's two-signature readmission —
/// the one path whose validity does not descend from current roster
/// membership — and a re-admission request that decoded to nothing would have
/// been carried forward as a key of length zero rather than refused.
///
/// The stricter reading wins, because the looser one is only ever right by
/// accident: every caller here is decoding something a person pasted, and an
/// empty paste is a mistake rather than a value.
pub fn unhex(s: &str) -> Option<alloc::vec::Vec<u8>> {
    if s.is_empty() || !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect()
}

/// Lowercase hex into a fixed-size array, or nothing.
///
/// The same fact as [`unhex`] with the length known in advance, which is how
/// four more call sites spelled it: a blob name into 32 bytes, an advertised
/// pseudonym into 8, a device id, a head. Each wrote its own loop, and each
/// had to remember to check the length first — the one place that forgot would
/// silently accept a short string and fill the rest with whatever `get`
/// returned, which is `None`, which is a refusal, which is fine. The point is
/// that it is fine *by accident* four times over.
pub fn unhex_array<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2 {
        return None;
    }
    let mut out = [0u8; N];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
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

#[cfg(test)]
mod hex_and_keys {
    use super::*;

    #[test]
    fn hex_round_trips() {
        for bytes in [
            &[][..],
            &[0x00][..],
            &[0xde, 0xad, 0xbe, 0xef][..],
            &[0xff; 32][..],
        ] {
            let text = hexed(bytes);
            assert_eq!(text.len(), bytes.len() * 2);
            assert_eq!(text, text.to_lowercase(), "hex is lowercase");
            if !bytes.is_empty() {
                assert_eq!(unhex(&text).as_deref(), Some(bytes));
            }
        }
    }

    /// The case the two copies disagreed on.
    ///
    /// One returned `None`, the other an empty vector — and they were used
    /// either side of §5.3's two-signature readmission, where a request
    /// decoding to nothing would have become a zero-length key rather than a
    /// refusal. Pinned here so a later merge cannot quietly pick the other
    /// behaviour back up.
    #[test]
    fn an_empty_string_is_not_zero_bytes() {
        assert_eq!(unhex(""), None);
    }

    #[test]
    fn what_is_not_hex_is_refused() {
        for s in ["a", "abc", "zz", "de ad", "0xdead", "DEADBEEG"] {
            assert_eq!(unhex(s), None, "{s:?} should not decode");
        }
    }

    #[test]
    fn uppercase_decodes_because_somebody_will_paste_it() {
        assert_eq!(unhex("DEADBEEF"), Some(alloc::vec![0xde, 0xad, 0xbe, 0xef]));
    }

    #[test]
    fn a_shown_key_is_always_the_same_length() {
        // Seven surfaces used to decide this independently. A key compared
        // against another key on a different screen has to be the same shape
        // on both, because comparing them is the entire point.
        let key = [0xab; 32];
        assert_eq!(short_key(&key).len(), SHOWN_KEY_BYTES * 2);
        assert_eq!(short_key(&key), "ab".repeat(SHOWN_KEY_BYTES));
    }

    #[test]
    fn a_short_key_does_not_panic_on_a_short_input() {
        // Every caller passes 32 bytes; nothing enforces it, and a slice panic
        // inside a display path would take down a surface for a cosmetic
        // reason.
        assert_eq!(short_key(&[0x01, 0x02]), "0102");
        assert_eq!(short_key(&[]), "");
    }
}
