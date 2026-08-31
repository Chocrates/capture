//! Who is allowed to author, and under which origin.
//!
//! # This is folded, but ingest depends on it
//!
//! Every other projection is read after the fact. The roster is read *during*
//! ingest, because verifying an event means finding the key its origin belongs
//! to — and that mapping is itself established by events.
//!
//! The circularity is real and is resolved by ordering rather than by
//! exception: a rebuild constructs the roster first, from the log, and ingest
//! is paused until it exists. The alternative — accepting events while the
//! roster is unavailable and checking them later — is not a weaker version of
//! the same guarantee. It is no guarantee, because by the time the check runs
//! the events already occupy positions.
//!
//! # Membership is keyed by public key; authorship is keyed by origin
//!
//! A device that is restored from a backup keeps its key and mints a **new
//! origin**, because reusing the old origin's sequence numbers would mean two
//! different events sharing one position. So the roster has two levels:
//!
//! - which **keys** are members, and
//! - which **origins** belong to each key.
//!
//! An origin with no entry is not merely unknown — its events cannot be
//! verified at all, since verification needs a key. Those events are held, not
//! discarded: the claim that binds the origin may simply not have arrived yet.
//!
//! # The self-signed root, and why it is bounded
//!
//! Membership events must be signed by an existing member, and the first device
//! is not one. The exception is stated narrowly enough to be checkable:
//!
//! > A self-signed admission counts as the root **only** as sequence 1 of an
//! > origin with no chain predecessor, and only for the first such event in
//! > the log.
//!
//! A second self-signed admission arriving later is authentic — it really was
//! signed by the key it names — so it keeps its position in its origin's
//! sequence. It simply never becomes a member. Rejecting it instead would
//! leave a permanent hole in that origin's numbering that nothing can fill.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::vec::Vec;

use crate::{
    Dev, Incarnation,
    body::{OriginClaimBody, RosterKeyBody},
    envelope::Envelope,
    event::Kind,
    id::EventId,
};

/// The membership state ingest consults.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Roster {
    /// Member keys.
    pub keys: BTreeSet<[u8; 32]>,
    /// Which key each origin authors under.
    pub origins: BTreeMap<Dev, [u8; 32]>,
    /// The shared secret discovery advertises under (§5.2), latest wins.
    ///
    /// `None` on an archive written before sync existed. That is not a defect
    /// to repair on read: such a device has never advertised, and minting a
    /// key is an event some device has to author.
    pub secret: Option<[u8; 32]>,
    /// Membership events that were authentic but did not confer membership,
    /// and why. Kept because "this device is silently not a member" is the
    /// failure that is impossible to diagnose without it.
    pub refused: BTreeMap<EventId, Refusal>,
}

/// Why an authentic membership event conferred nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// A self-signed admission ordered after the log's root. Which of two
    /// competing roots wins is decided by timestamp, tiebroken by origin, so
    /// every replica refuses the same one.
    NotTheFirstRoot,
    /// A self-signed admission that was not sequence 1 of a fresh origin.
    NotRootShaped,
    /// The origin it claims is not the one its key and incarnation produce, so
    /// admitting it would let any member author under any origin name.
    OriginDoesNotMatchKey,
    /// It claims an origin for a key that is not a member.
    KeyIsNotAMember,
}

impl Roster {
    /// The key an origin authors under, if the roster knows.
    pub fn key_for(&self, dev: &Dev) -> Option<&[u8; 32]> {
        self.origins.get(dev)
    }

    /// Whether the roster has a root at all.
    ///
    /// False means ingest cannot verify anything and must wait rather than
    /// accept — see the module comment.
    pub fn is_bootstrapped(&self) -> bool {
        !self.keys.is_empty()
    }
}

/// Build the roster from a log.
///
/// Events are considered in timestamp order so the result does not depend on
/// how they arrived. Signatures are **not** checked here: this is pure and has
/// no access to verification, and the caller checks each event against the key
/// the roster hands back. What this decides is narrower — which key an origin
/// is entitled to be checked against.
pub fn build(events: &[Envelope]) -> Roster {
    let mut ordered: Vec<&Envelope> = events
        .iter()
        .filter(|e| {
            matches!(
                e.header.kind,
                Kind::DEVICE_ADD | Kind::ORIGIN_CLAIM | Kind::ROSTER_KEY
            )
        })
        .collect();
    ordered.sort_by_key(|e| e.header.hlc);

    let mut r = Roster::default();
    let mut root_seen = false;

    for e in ordered {
        let id = e.header.id();
        let Some(body) = e.body.as_deref() else {
            // A redacted membership event. Nothing to read, and nothing that
            // can be inferred from its absence.
            continue;
        };
        if e.header.kind == Kind::ROSTER_KEY {
            // Ordered by timestamp, so the last rotation any replica has seen
            // is the one it advertises under. A device still holding an older
            // key fails to recognise its peers until the rotation reaches it,
            // which is a delay in being found rather than a divergence.
            if !r.origins.contains_key(&e.header.dev) {
                // Any member may rotate; a non-member minting the secret would
                // be an outsider choosing what the mesh advertises under.
                r.refused.insert(id, Refusal::KeyIsNotAMember);
                continue;
            }
            if let Ok(rekey) = minicbor::decode::<RosterKeyBody>(body) {
                r.secret = Some(rekey.secret);
            }
            continue;
        }

        let Ok(claim) = minicbor::decode::<OriginClaimBody>(body) else {
            continue;
        };

        // Both kinds assert "this origin belongs to this key". An origin is a
        // function of the key and the incarnation, so the assertion is
        // checkable against itself — and must be, or a member could name any
        // origin it liked, including one already in use by another device.
        if Dev::derive(&claim.pubkey, &Incarnation(claim.incarnation)) != e.header.dev {
            r.refused.insert(id, Refusal::OriginDoesNotMatchKey);
            continue;
        }

        match e.header.kind {
            Kind::DEVICE_ADD => {
                if e.header.seq != 1 || e.header.chain.is_some() {
                    r.refused.insert(id, Refusal::NotRootShaped);
                    continue;
                }
                if root_seen {
                    r.refused.insert(id, Refusal::NotTheFirstRoot);
                    continue;
                }
                root_seen = true;
                r.keys.insert(claim.pubkey);
                r.origins.insert(e.header.dev, claim.pubkey);
            }
            Kind::ORIGIN_CLAIM => {
                if !r.keys.contains(&claim.pubkey) {
                    // Possibly premature rather than wrong — the admission may
                    // still be in flight. The caller re-evaluates on every
                    // membership arrival and on every rebuild.
                    r.refused.insert(id, Refusal::KeyIsNotAMember);
                    continue;
                }
                r.origins.insert(e.header.dev, claim.pubkey);
            }
            _ => {}
        }
    }

    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;

    #[test]
    fn the_first_self_signed_admission_is_the_root() {
        let mut d = testing::Origin::new(0);
        let root = d.roster_root();
        let r = build(&[root]);
        assert!(r.is_bootstrapped());
        assert_eq!(r.key_for(&d.dev()), Some(&d.pubkey()));
    }

    #[test]
    fn an_empty_log_is_not_bootstrapped() {
        // The state in which ingest must pause rather than accept.
        assert!(!build(&[]).is_bootstrapped());
    }

    #[test]
    fn only_one_of_two_competing_roots_becomes_a_member() {
        // Which one wins is decided by the timestamp order, tiebroken by
        // origin — deliberately not by arrival, so every replica reaches the
        // same answer. The test therefore asserts the shape of the outcome
        // rather than naming the winner.
        let mut a = testing::Origin::new(0);
        let mut b = testing::Origin::new(1);
        let first = a.roster_root();
        let second = b.roster_root();
        let ids = [first.header.id(), second.header.id()];

        let r = build(&[first, second]);
        assert_eq!(r.keys.len(), 1, "only one key is admitted");
        assert_eq!(r.origins.len(), 1);
        let loser = ids
            .iter()
            .find(|i| r.refused.contains_key(i))
            .expect("the other root must be refused, not silently dropped");
        assert_eq!(r.refused.get(loser), Some(&Refusal::NotTheFirstRoot));
    }

    #[test]
    fn which_root_wins_does_not_depend_on_arrival_order() {
        let mut a = testing::Origin::new(0);
        let mut b = testing::Origin::new(1);
        let first = a.roster_root();
        let second = b.roster_root();

        let forward = build(&[first.clone(), second.clone()]);
        let backward = build(&[second, first]);
        assert_eq!(forward.keys, backward.keys);
        assert_eq!(forward.origins, backward.origins);
    }

    #[test]
    fn a_restore_binds_its_new_origin_to_the_same_key() {
        let mut first = testing::Origin::new(0);
        let root = first.roster_root();
        let mut restored = first.restored([9u8; 16]);
        let claim = restored.origin_claim();

        let r = build(&[root, claim]);
        assert_eq!(r.keys.len(), 1, "a restore does not add a member");
        assert_eq!(r.key_for(&first.dev()), Some(&first.pubkey()));
        assert_eq!(
            r.key_for(&restored.dev()),
            Some(&first.pubkey()),
            "the new origin authors under the original key"
        );
    }

    #[test]
    fn a_claim_from_a_stranger_confers_nothing() {
        let mut member = testing::Origin::new(0);
        let root = member.roster_root();
        let mut stranger = testing::Origin::new(5);
        let claim = stranger.origin_claim();
        let claim_id = claim.header.id();

        let r = build(&[root, claim]);
        assert!(r.key_for(&stranger.dev()).is_none());
        assert_eq!(r.refused.get(&claim_id), Some(&Refusal::KeyIsNotAMember));
    }

    #[test]
    fn a_claim_that_arrives_before_its_admission_is_picked_up_on_rebuild() {
        // The re-evaluation case. Ordering is by timestamp, so a claim that
        // reached this device first still lands behind the admission it needs
        // once both are present.
        let mut first = testing::Origin::new(0);
        let root = first.roster_root();
        let mut restored = first.restored([9u8; 16]);
        let claim = restored.origin_claim();

        let early = build(core::slice::from_ref(&claim));
        assert!(
            early.key_for(&restored.dev()).is_none(),
            "nothing to check it against yet"
        );

        let later = build(&[claim, root]);
        assert_eq!(later.key_for(&restored.dev()), Some(&first.pubkey()));
    }

    #[test]
    fn an_origin_that_does_not_follow_from_the_key_is_refused() {
        // Without this check a member could author under an origin name it
        // does not own — including one another device is already using.
        let mut d = testing::Origin::new(0);
        let mut root = d.roster_root();
        // Rewrite the body to name a different incarnation than the one the
        // origin was derived from.
        let mut body: OriginClaimBody = minicbor::decode(root.body.as_ref().unwrap()).unwrap();
        body.incarnation = [0xAA; 16];
        let mut buf = alloc::vec::Vec::new();
        minicbor::encode(&body, &mut buf).unwrap();
        root.header.bhash = crate::id::BodyHash::of(&buf);
        root.body = Some(buf);
        let id = root.header.id();

        let r = build(&[root]);
        assert!(!r.is_bootstrapped());
        assert_eq!(r.refused.get(&id), Some(&Refusal::OriginDoesNotMatchKey));
    }

    #[test]
    fn a_root_with_a_predecessor_is_not_a_root() {
        let mut d = testing::Origin::new(0);
        let mut root = d.roster_root();
        root.header.chain = Some(EventId([7u8; 32]));
        let id = root.header.id();

        let r = build(&[root]);
        assert!(!r.is_bootstrapped());
        assert_eq!(r.refused.get(&id), Some(&Refusal::NotRootShaped));
    }

    #[test]
    fn ordinary_events_do_not_affect_membership() {
        let s = testing::mixed(3, 2, 3, 12);
        assert!(!build(&s.events).is_bootstrapped());
    }
}
