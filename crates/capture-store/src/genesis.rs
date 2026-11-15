//! The first events a device authors: the roster root, and origin claims.
//!
//! Both answer the same question — *which public key is allowed to author under
//! which origin* — and both exist because verification needs an answer before
//! it can accept anything.
//!
//! These functions **author** events; they do not store them. Storage arrives
//! with the event log itself. Keeping authoring separate means the rules here
//! are testable without a database.

use capture_core::{
    Dev, Incarnation, Nonce,
    body::{OriginClaimBody, RosterKeyBody},
    envelope::Envelope,
    event::{ENVELOPE_VERSION, Header, Kind},
    hlc::HlcState,
    id::{BodyHash, EventId},
};
use ed25519_dalek::{Signer, SigningKey};

use crate::Result;

/// Author the roster root: a device admitting itself.
///
/// # The exception this represents
///
/// Every other membership event must be signed by an existing member. The first
/// device is not yet a member, so the rule has to bend once — and the bend is
/// stated narrowly rather than left implicit:
///
/// > A self-signed admission is valid **only** as sequence 1 of an origin with
/// > no predecessor, in a log containing no other root.
///
/// That is checkable. The alternative phrasing — "an empty roster means trust
/// whatever is local" — has no such bound, and an unbounded exception in
/// membership checking is how a second, attacker-authored root gets accepted
/// later.
///
/// A second root arriving afterwards is **quarantined, not rejected**: its
/// signature verifies, so it legitimately occupies its position in its origin's
/// sequence. It simply never joins the roster. Rejecting it outright would
/// leave a hole in that origin's numbering, which nothing can fill.
pub fn roster_root(
    signing: &SigningKey,
    incarnation: &Incarnation,
    clock: &mut HlcState,
    physical_now: u64,
    nonce: Nonce,
    noise: [u8; 32],
) -> Result<Envelope> {
    let pubkey = signing.verifying_key().to_bytes();
    let dev = Dev::derive(&pubkey, incarnation);

    let body = OriginClaimBody {
        nonce: nonce.0,
        pubkey,
        incarnation: incarnation.0,
        // The key a peer opens a Noise session with. Published here because a
        // membership event is where a device states what it is.
        noise: Some(noise),
        role: None,
        name: None,
    };
    let body_bytes = encode(&body);

    let hlc = clock.issue(physical_now, dev);

    let header = Header {
        v: ENVELOPE_VERSION,
        dev,
        seq: 1,
        hlc,
        kind: Kind::DEVICE_ADD,
        tgt: None,
        prev: None,
        occ: None,
        // No predecessor. This is part of what makes the root checkable: a
        // self-signed admission claiming a chain link is not a root.
        chain: None,
        bhash: BodyHash::of(&body_bytes),
    };

    Ok(sign(header, body_bytes, signing))
}

/// Author an origin claim: an existing key declaring a new origin for itself.
///
/// # Why a restore needs this
///
/// Membership is keyed by **public key**, so a restored device is still a
/// member. But a restore also mints a new incarnation, and therefore a new
/// origin — and nothing else in the log says which key that new origin belongs
/// to.
///
/// Without the claim, every device that later sees events under the new origin
/// finds no key for it and refuses them permanently. The device would be a
/// roster member that cannot author anything. Worse, nothing detects it
/// locally, because local verification uses the local key: the failure surfaces
/// only once a second device exists, in events that are immutable by then.
///
/// So the claim is the **first** event under a new origin, before anything else
/// is written under it.
pub fn origin_claim(
    signing: &SigningKey,
    incarnation: &Incarnation,
    clock: &mut HlcState,
    physical_now: u64,
    nonce: Nonce,
    noise: [u8; 32],
) -> Result<Envelope> {
    let pubkey = signing.verifying_key().to_bytes();
    let dev = Dev::derive(&pubkey, incarnation);

    let body = OriginClaimBody {
        nonce: nonce.0,
        pubkey,
        incarnation: incarnation.0,
        // The key a peer opens a Noise session with. Published here because a
        // membership event is where a device states what it is.
        noise: Some(noise),
        role: None,
        name: None,
    };
    let body_bytes = encode(&body);

    let hlc = clock.issue(physical_now, dev);

    let header = Header {
        v: ENVELOPE_VERSION,
        dev,
        seq: 1,
        hlc,
        kind: Kind::ORIGIN_CLAIM,
        tgt: None,
        prev: None,
        occ: None,
        chain: None,
        bhash: BodyHash::of(&body_bytes),
    };

    Ok(sign(header, body_bytes, signing))
}

/// Author a `roster_key`: mint or rotate the secret the mesh advertises under.
///
/// Minting and rotating are the same event. The first one is authored right
/// after the roster root, which is why this takes a `seq` and a `chain` rather
/// than assuming a fresh origin the way the two above do — every roster_key
/// after the first has a predecessor.
///
/// Rotation is what a retirement is followed by (§5.2): the retired device
/// holds the whole log and can therefore go on deriving anything derivable
/// from what it already has, but it cannot receive the replacement, because
/// ingesting the retirement tears its sessions down.
#[allow(clippy::too_many_arguments)]
pub fn roster_key(
    signing: &SigningKey,
    incarnation: &Incarnation,
    clock: &mut HlcState,
    physical_now: u64,
    nonce: Nonce,
    secret: [u8; 32],
    seq: u64,
    chain: Option<EventId>,
) -> Result<Envelope> {
    let pubkey = signing.verifying_key().to_bytes();
    let dev = Dev::derive(&pubkey, incarnation);

    let body = RosterKeyBody {
        nonce: nonce.0,
        secret,
    };
    let body_bytes = encode(&body);

    let hlc = clock.issue(physical_now, dev);

    let header = Header {
        v: ENVELOPE_VERSION,
        dev,
        seq,
        hlc,
        kind: Kind::ROSTER_KEY,
        tgt: None,
        prev: None,
        occ: None,
        chain,
        bhash: BodyHash::of(&body_bytes),
    };

    Ok(sign(header, body_bytes, signing))
}

fn encode<T: minicbor::Encode<()>>(v: &T) -> Vec<u8> {
    let mut b = Vec::new();
    minicbor::encode(v, &mut b).expect("encoding into Vec is infallible");
    b
}

fn sign(header: Header, body: Vec<u8>, signing: &SigningKey) -> Envelope {
    let sig = signing.sign(&header.id().0).to_bytes();
    Envelope {
        header,
        body: Some(body),
        sig,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use capture_core::envelope::Verdict;

    fn key(b: u8) -> SigningKey {
        SigningKey::from_bytes(&[b; 32])
    }

    #[test]
    fn a_roster_root_verifies_against_its_own_key() {
        let k = key(1);
        let inc = Incarnation([2u8; 16]);
        let mut c = HlcState::new();
        let e = roster_root(&k, &inc, &mut c, 1000, Nonce([3u8; 16]), [0x44; 32]).unwrap();
        assert_eq!(e.verify(&k.verifying_key().to_bytes()), Verdict::Authentic);
    }

    #[test]
    fn a_root_is_the_first_event_with_no_predecessor() {
        // Both conditions are part of what makes the self-signing exception
        // checkable rather than open-ended.
        let k = key(1);
        let e = roster_root(
            &k,
            &Incarnation([2u8; 16]),
            &mut HlcState::new(),
            1000,
            Nonce([3u8; 16]),
            [0x44; 32],
        )
        .unwrap();
        assert_eq!(e.header.seq, 1);
        assert!(e.header.chain.is_none());
    }

    #[test]
    fn a_root_carries_the_key_it_admits() {
        let k = key(9);
        let e = roster_root(
            &k,
            &Incarnation([1u8; 16]),
            &mut HlcState::new(),
            1000,
            Nonce([0u8; 16]),
            [0x44; 32],
        )
        .unwrap();
        let body: OriginClaimBody = minicbor::decode(e.body.as_ref().unwrap()).unwrap();
        assert_eq!(body.pubkey, k.verifying_key().to_bytes());
    }

    #[test]
    fn an_origin_claim_binds_a_new_origin_to_the_same_key() {
        // The restore case: same key, new incarnation, new origin.
        let k = key(4);
        let first = Incarnation([1u8; 16]);
        let second = Incarnation([2u8; 16]);

        let root = roster_root(
            &k,
            &first,
            &mut HlcState::new(),
            1000,
            Nonce([0u8; 16]),
            [0x44; 32],
        )
        .unwrap();
        let claim = origin_claim(
            &k,
            &second,
            &mut HlcState::new(),
            2000,
            Nonce([1u8; 16]),
            [0x44; 32],
        )
        .unwrap();

        assert_ne!(
            root.header.dev, claim.header.dev,
            "a restore is a new origin"
        );

        let rb: OriginClaimBody = minicbor::decode(root.body.as_ref().unwrap()).unwrap();
        let cb: OriginClaimBody = minicbor::decode(claim.body.as_ref().unwrap()).unwrap();
        assert_eq!(rb.pubkey, cb.pubkey, "and the same roster member");
    }

    #[test]
    fn a_claim_verifies_against_the_roster_key() {
        // The property that makes restore recoverable: any peer holding the
        // key from the original roster entry can check the claim.
        let k = key(4);
        let claim = origin_claim(
            &k,
            &Incarnation([7u8; 16]),
            &mut HlcState::new(),
            1000,
            Nonce([0u8; 16]),
            [0x44; 32],
        )
        .unwrap();
        assert_eq!(
            claim.verify(&k.verifying_key().to_bytes()),
            Verdict::Authentic
        );
    }

    #[test]
    fn a_claim_is_the_first_event_under_its_origin() {
        let claim = origin_claim(
            &key(4),
            &Incarnation([7u8; 16]),
            &mut HlcState::new(),
            1000,
            Nonce([0u8; 16]),
            [0x44; 32],
        )
        .unwrap();
        assert_eq!(claim.header.seq, 1);
    }

    #[test]
    fn a_foreign_key_does_not_verify_a_root() {
        let e = roster_root(
            &key(1),
            &Incarnation([2u8; 16]),
            &mut HlcState::new(),
            1000,
            Nonce([3u8; 16]),
            [0x44; 32],
        )
        .unwrap();
        assert!(matches!(
            e.verify(&key(2).verifying_key().to_bytes()),
            Verdict::Rejected(_)
        ));
    }
}
