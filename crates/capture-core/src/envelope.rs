//! A complete event: header, body bytes, signature — and how to verify one.
//!
//! # Why verification has three parts
//!
//! `sig` covers `id`, and `id` covers the header, and the header carries only a
//! *hash* of the body. Each link has to be checked or the chain has a hole:
//!
//! 1. `id == blake3(canonical header)` — the header is what it claims to be.
//! 2. `sig` verifies against `id` under the origin's key — the origin authored it.
//! 3. `blake3(body) == bhash` — **the body is the one that was signed.**
//!
//! Step 3 is the one that looks redundant and is not. Without it a relaying
//! device can swap the body for different content: the id still recomputes, the
//! signature still verifies, and two replicas end up holding permanently
//! different content under the same id, deduplicating each other's copies on
//! sight. Events are relayed by design, so this is precisely the attack
//! per-event signatures exist to prevent.

use alloc::vec::Vec;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use minicbor::{Decode, Encode};

use crate::{
    event::Header,
    id::{BodyHash, EventId},
};

/// A stored event.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct Envelope {
    /// The signed header.
    #[n(0)]
    pub header: Header,
    /// Canonical body bytes. `None` once redacted — the row survives, and
    /// because `bhash` survives with it, the event still verifies.
    #[cbor(n(1), with = "minicbor::bytes")]
    pub body: Option<Vec<u8>>,
    /// Ed25519 over the event id.
    #[cbor(n(2), with = "minicbor::bytes")]
    pub sig: [u8; 64],
}

/// Why an event was not accepted.
///
/// The split matters more than it looks. A **rejected** event is
/// unauthenticated: it must never occupy a `(dev, seq)` position or advance
/// `heads`, because allowing that lets any peer squat a sequence number, after
/// which the genuine event at that position can never be accepted anywhere.
///
/// A **quarantined** event is authentic but unfoldable — an unknown kind, a
/// missing chain predecessor. It is stored in place and relayed, because
/// dropping it would leave a gap, and a version vector cannot represent a hole.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Rejection {
    /// The id does not match the header it claims to cover.
    IdMismatch,
    /// The signature does not verify under the origin's key.
    BadSignature,
    /// The body does not hash to the commitment in the signed header.
    BodyMismatch,
    /// The origin's public key is not known.
    UnknownOrigin,
}

/// Outcome of verifying an event.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Authentic. May occupy its position.
    Authentic,
    /// Unauthenticated. Must not occupy a position or advance `heads`.
    Rejected(Rejection),
}

impl Envelope {
    /// This event's id, recomputed from the header.
    pub fn id(&self) -> EventId {
        self.header.id()
    }

    /// Full authenticity check.
    ///
    /// `pubkey` is the origin's roster key. Supplied by the caller because the
    /// roster is a fold output and this crate does not read storage.
    pub fn verify(&self, pubkey: &[u8; 32]) -> Verdict {
        let key = match VerifyingKey::from_bytes(pubkey) {
            Ok(k) => k,
            Err(_) => return Verdict::Rejected(Rejection::UnknownOrigin),
        };

        let id = self.header.id();
        let sig = Signature::from_bytes(&self.sig);
        if key.verify(&id.0, &sig).is_err() {
            return Verdict::Rejected(Rejection::BadSignature);
        }

        // Step 3. Skipped only when the body is genuinely gone — a redacted
        // event has nothing to hash, and that is a known, visible state rather
        // than a silent integrity failure.
        if let Some(body) = &self.body
            && BodyHash::of(body) != self.header.bhash
        {
            return Verdict::Rejected(Rejection::BodyMismatch);
        }

        Verdict::Authentic
    }

    /// Whether this event has been redacted.
    pub fn is_redacted(&self) -> bool {
        self.body.is_none()
    }
}

/// Verify one origin's hash chain over a run of consecutive events.
///
/// The chain is what makes an origin's history verifiable **independently of
/// any timestamp the origin asserts about itself** — which matters because a
/// device chooses its own clock and can backdate freely.
///
/// It detects removal, truncation and substitution. It does **not** detect
/// reordering, and cannot: `chain` is a property of the links, not of storage
/// order, and arrival order is arbitrary by construction.
pub fn verify_chain(run: &[Envelope]) -> Result<(), ChainBreak> {
    for pair in run.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if b.header.seq != a.header.seq + 1 {
            return Err(ChainBreak::Gap {
                after: a.header.seq,
            });
        }
        match b.header.chain {
            Some(link) if link == a.id() => {}
            _ => {
                return Err(ChainBreak::Broken { at: b.header.seq });
            }
        }
    }
    Ok(())
}

/// How a chain failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainBreak {
    /// A sequence number is missing.
    Gap {
        /// Last sequence before the gap.
        after: u64,
    },
    /// An event does not link to its predecessor.
    Broken {
        /// Sequence at which the link failed.
        at: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Incarnation,
        body::{Ambient, CaptureBody},
        event::{ENVELOPE_VERSION, Kind},
        hlc::Hlc,
        id::Dev,
    };
    use alloc::string::ToString;
    use ed25519_dalek::{Signer, SigningKey};

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[42u8; 32])
    }

    fn body_bytes(nonce: u8, text: &str) -> Vec<u8> {
        let b = CaptureBody {
            nonce: [nonce; 16],
            text: text.to_string(),
            tz: None,
            ambient: Ambient::default(),
            blob: None,
        };
        let mut out = Vec::new();
        minicbor::encode(&b, &mut out).unwrap();
        out
    }

    fn event(seq: u64, chain: Option<EventId>, text: &str) -> Envelope {
        let sk = signing_key();
        let pk = sk.verifying_key().to_bytes();
        let dev = Dev::derive(&pk, &Incarnation([1u8; 16]));
        let body = body_bytes(seq as u8, text);
        let header = Header {
            v: ENVELOPE_VERSION,
            dev,
            seq,
            hlc: Hlc {
                wall: 1000 + seq,
                counter: 0,
                origin: dev,
            },
            kind: Kind::CAPTURE,
            tgt: None,
            prev: None,
            occ: None,
            chain,
            bhash: BodyHash::of(&body),
        };
        let sig = sk.sign(&header.id().0).to_bytes();
        Envelope {
            header,
            body: Some(body),
            sig,
        }
    }

    fn pubkey() -> [u8; 32] {
        signing_key().verifying_key().to_bytes()
    }

    #[test]
    fn a_well_formed_event_verifies() {
        assert_eq!(
            event(1, None, "hello").verify(&pubkey()),
            Verdict::Authentic
        );
    }

    #[test]
    fn substituted_body_is_caught() {
        // The attack per-event signatures exist to stop: a relay swaps the body.
        // id still recomputes, signature still verifies — only the bhash check
        // catches it.
        let mut e = event(1, None, "budget 40k");
        e.body = Some(body_bytes(1, "budget 400k"));
        assert_eq!(
            e.verify(&pubkey()),
            Verdict::Rejected(Rejection::BodyMismatch)
        );
    }

    #[test]
    fn tampered_header_is_caught() {
        let mut e = event(1, None, "hello");
        e.header.seq = 99; // id changes, signature no longer matches
        assert_eq!(
            e.verify(&pubkey()),
            Verdict::Rejected(Rejection::BadSignature)
        );
    }

    #[test]
    fn a_different_key_does_not_verify() {
        let other = SigningKey::from_bytes(&[7u8; 32])
            .verifying_key()
            .to_bytes();
        assert!(matches!(
            event(1, None, "hello").verify(&other),
            Verdict::Rejected(_)
        ));
    }

    #[test]
    fn a_redacted_event_still_verifies() {
        // The whole reason id commits to bhash rather than to the body.
        let mut e = event(1, None, "secret");
        e.body = None;
        assert_eq!(e.verify(&pubkey()), Verdict::Authentic);
        assert!(e.is_redacted());
    }

    #[test]
    fn chain_accepts_an_intact_run() {
        let a = event(1, None, "one");
        let b = event(2, Some(a.id()), "two");
        let c = event(3, Some(b.id()), "three");
        assert!(verify_chain(&[a, b, c]).is_ok());
    }

    #[test]
    fn chain_detects_truncation() {
        let a = event(1, None, "one");
        let b = event(2, Some(a.id()), "two");
        let c = event(3, Some(b.id()), "three");
        // b removed
        assert!(matches!(
            verify_chain(&[a, c]),
            Err(ChainBreak::Gap { after: 1 })
        ));
    }

    #[test]
    fn chain_detects_substitution() {
        let a = event(1, None, "one");
        let b = event(2, Some(a.id()), "two");
        let forged = event(2, Some(EventId([0u8; 32])), "forged");
        let c = event(3, Some(b.id()), "three");
        assert!(matches!(
            verify_chain(&[a, forged, c]),
            Err(ChainBreak::Broken { at: 2 })
        ));
    }
}
