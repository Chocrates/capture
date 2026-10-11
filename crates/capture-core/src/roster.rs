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
    body::{OriginClaimBody, ReadmitBody, RetireBody, RosterKeyBody, readmit_digest},
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
    /// Keys that have been retired, and the last event of each of their
    /// origins that is to be honoured.
    ///
    /// Kept rather than removed. A retired device's past events remain valid —
    /// they were legitimately authored, and discarding them would mean losing
    /// real notes — so what a retirement changes is where an origin *stops*,
    /// not whether it ever existed.
    pub retired: BTreeMap<[u8; 32], BTreeMap<Dev, (u64, EventId)>>,
    /// Who issued each retirement.
    ///
    /// Recorded because "a retirement of myself that I did not issue" is not
    /// answerable from the retirement alone, and that distinction is the first
    /// of the three lockout guards (§5.3).
    pub retired_by: BTreeMap<[u8; 32], BTreeSet<[u8; 32]>>,
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
    /// When each key first became a member: the clock of the root, the
    /// `device_add` or the re-admission that first put it here.
    ///
    /// *First*, not latest. Pairing writes two admissions — each side admits
    /// the other — and the second would otherwise make the device that was
    /// already here look like the newer one. It is what §2.4.1's default
    /// notifier, "the most recently paired device", is ordered by, and every
    /// replica folds the same membership events, so every replica agrees on it
    /// without anything being written.
    pub admitted: BTreeMap<[u8; 32], crate::hlc::Hlc>,
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
    /// A retirement that would leave no members at all.
    ///
    /// Refused locally rather than folded, because a roster with nobody in it
    /// can never admit anyone again: `device_add` needs a signature from a
    /// current member.
    WouldEmptyTheRoster,
    /// A retirement or re-admission naming a key the roster does not hold.
    NotAMember,
    /// A re-admission whose second signature does not check out.
    ///
    /// The co-signature is the entire difference between this and an ordinary
    /// admission — it is what makes forging one require two compromised
    /// devices rather than one.
    BadCosignature,
    /// A re-admission signed twice by the same key.
    ///
    /// Two signatures from one device are one device's say-so written twice.
    NotTwoSigners,
    /// An admission for a key that has been retired.
    ///
    /// Retirement wins over a concurrent admission, deliberately the opposite
    /// of the add-wins rule used for tags: for content, losing data is the
    /// worse outcome, so adds win; for access, granting unintended access is
    /// the worse outcome, so revocation wins. Re-admission is the deliberate
    /// route back, and it needs two signers.
    RetiredKey,
    /// An admission signed by an origin the roster does not know.
    ///
    /// Usually premature rather than hostile: the admitter's own admission may
    /// still be in flight. Re-evaluated on every membership arrival, like the
    /// others.
    AdmitterIsNotAMember,
}

impl Roster {
    /// The key an origin authors under, if the roster knows.
    pub fn key_for(&self, dev: &Dev) -> Option<&[u8; 32]> {
        self.origins.get(dev)
    }

    /// Undo a retirement of this device that this device did not issue.
    ///
    /// The first lockout guard (§5.3). Any member may retire any member, so a
    /// compromised or malfunctioning device can retire every other one, after
    /// which nothing honest can be re-admitted without the two-signature
    /// route. A device therefore does not accept being retired on someone
    /// else's say-so alone.
    ///
    /// This deliberately makes replicas disagree: the retired device still
    /// considers itself a member while its peers do not. That is the point —
    /// the alternative is a device that has locked itself out on the strength
    /// of one event it had no part in. A stolen device is unaffected, because
    /// what retiring it achieves is that its *peers* stop accepting its
    /// events, which they still do.
    pub fn honour_self(&mut self, own: &[u8; 32]) {
        let issued_by_me = self
            .retired_by
            .get(own)
            .is_some_and(|issuers| issuers.contains(own));
        if self.retired.contains_key(own) && !issued_by_me {
            self.retired.remove(own);
            self.retired_by.remove(own);
            self.keys.insert(*own);
        }
    }

    /// Where a retired origin stops, if it is retired at all.
    pub fn head_of(&self, dev: &Dev) -> Option<(u64, EventId)> {
        let key = self.origins.get(dev)?;
        self.retired.get(key)?.get(dev).copied()
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
                Kind::DEVICE_ADD
                    | Kind::ORIGIN_CLAIM
                    | Kind::ROSTER_KEY
                    | Kind::DEVICE_RETIRE
                    | Kind::DEVICE_READMIT
            )
        })
        .collect();
    ordered.sort_by_key(|e| e.header.hlc);

    // Which retirements have been replaced by a reissue. Collected before
    // anything is applied, so the outcome does not depend on the order the two
    // arrived in — a replica that saw the reissue first must reach the same
    // roster as one that saw it second.
    let mut superseded = BTreeSet::new();
    for e in &ordered {
        if e.header.kind == Kind::DEVICE_RETIRE
            && let Some(body) = e.body.as_deref()
            && let Ok(b) = minicbor::decode::<RetireBody>(body)
            && let Some(old) = b.supersedes
        {
            superseded.insert(old);
        }
    }

    let mut r = Roster::default();
    let mut root_seen = false;

    for e in ordered {
        let id = e.header.id();
        let Some(body) = e.body.as_deref() else {
            // A redacted membership event. Nothing to read, and nothing that
            // can be inferred from its absence.
            continue;
        };
        if e.header.kind == Kind::DEVICE_RETIRE {
            let Ok(body) = minicbor::decode::<RetireBody>(body) else {
                continue;
            };
            if !r.origins.contains_key(&e.header.dev) {
                r.refused.insert(id, Refusal::AdmitterIsNotAMember);
                continue;
            }
            if superseded.contains(&id) {
                // Replaced by a reissue naming a different head. Applying it
                // would let the older, narrower claim win by arriving second.
                continue;
            }
            let is_member = r.keys.contains(&body.pubkey);
            let already_retired = r.retired.contains_key(&body.pubkey);
            if !is_member && !already_retired {
                r.refused.insert(id, Refusal::NotAMember);
                continue;
            }
            if is_member && r.keys.len() == 1 {
                // The last member cannot retire itself into a roster nobody
                // can ever join.
                r.refused.insert(id, Refusal::WouldEmptyTheRoster);
                continue;
            }
            // A second retirement of a key already retired is not redundant:
            // it is how the lowest head wins. Refusing it as "not a member"
            // would defeat the only rule that keeps two replicas agreeing
            // about where a retired origin stops.

            r.keys.remove(&body.pubkey);
            if let Some(issuer) = r.origins.get(&e.header.dev).copied() {
                r.retired_by.entry(body.pubkey).or_default().insert(issuer);
            }
            let reissue = body.supersedes.is_some();
            let heads = r.retired.entry(body.pubkey).or_default();
            for h in body.heads {
                if reissue {
                    // A deliberate decision about what to keep, replacing
                    // whatever the retirement it names had said.
                    heads.insert(h.dev, (h.seq, h.last));
                    continue;
                }
                heads
                    .entry(h.dev)
                    .and_modify(|held| {
                        // Two devices retiring the same stolen phone will each
                        // name whatever they had seen. Lowest wins, per
                        // origin: it is the safe direction, excluding the
                        // events whose provenance is in doubt. Without a rule
                        // the two replicas permanently disagree about which
                        // events exist, and nothing would flag it —
                        // `divergences` tracks note bodies only.
                        if h.seq < held.0 {
                            *held = (h.seq, h.last);
                        }
                    })
                    .or_insert((h.seq, h.last));
            }
            continue;
        }

        if e.header.kind == Kind::DEVICE_READMIT {
            let Ok(body) = minicbor::decode::<ReadmitBody>(body) else {
                continue;
            };
            if body.cosigner == body.pubkey {
                // The subject vouching for itself is one device's say-so.
                r.refused.insert(id, Refusal::NotTwoSigners);
                continue;
            }
            let digest = readmit_digest(&body.pubkey, &body.incarnation, &body.nonce);
            let ok = ed25519_dalek::VerifyingKey::from_bytes(&body.cosigner)
                .ok()
                .and_then(|k| {
                    let sig = ed25519_dalek::Signature::from_bytes(&body.cosign);
                    k.verify_strict(&digest, &sig).ok()
                })
                .is_some();
            if !ok {
                r.refused.insert(id, Refusal::BadCosignature);
                continue;
            }

            // Valid even when both signers are retired: in a lockout every
            // honest device has been, and requiring membership here would
            // make the remedy unusable exactly when it is needed.
            r.retired.remove(&body.pubkey);
            r.retired_by.remove(&body.pubkey);
            r.keys.insert(body.pubkey);
            r.admitted.entry(body.pubkey).or_insert(e.header.hlc);
            r.origins.insert(
                Dev::derive(&body.pubkey, &Incarnation(body.incarnation)),
                body.pubkey,
            );
            continue;
        }

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

        // The origin the body names. An origin is a function of a key and an
        // incarnation, so "this origin belongs to this key" is checkable
        // against itself — and must be, or a member could name any origin it
        // liked, including one already in use by another device.
        let subject = Dev::derive(&claim.pubkey, &Incarnation(claim.incarnation));
        let self_signed = subject == e.header.dev;

        match e.header.kind {
            // Two shapes, distinguished by who signed it.
            //
            // Self-signed is the root: a device admitting itself, which is only
            // meaningful once, at the start of an archive. Signed by somebody
            // else, it is an admission — the event a pairing produces, and the
            // one that makes the roster O(n) rather than O(n²): every other
            // device learns the newcomer by folding this rather than by
            // pairing with it (§5.3).
            Kind::DEVICE_ADD if self_signed => {
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
                r.admitted.entry(claim.pubkey).or_insert(e.header.hlc);
                r.origins.insert(e.header.dev, claim.pubkey);
            }
            Kind::DEVICE_ADD => {
                // Any member may admit; there is no admin device, because a
                // designated admin is the single point of failure serverless
                // exists to avoid (§5.3). What is checked here is only that
                // the *admitter* is a member — the caller verifies the
                // signature against the key this hands back for that origin,
                // so an outsider cannot sign one at all.
                if !r.origins.contains_key(&e.header.dev) {
                    r.refused.insert(id, Refusal::AdmitterIsNotAMember);
                    continue;
                }
                if r.retired.contains_key(&claim.pubkey) {
                    // Retire wins, whatever the order. A retirement that could
                    // be undone by an ordinary admission would mean a stolen
                    // device merely had to be re-added by any member — or by
                    // itself, once it had compromised one.
                    r.refused.insert(id, Refusal::RetiredKey);
                    continue;
                }
                r.keys.insert(claim.pubkey);
                r.admitted.entry(claim.pubkey).or_insert(e.header.hlc);
                r.origins.insert(subject, claim.pubkey);
            }
            Kind::ORIGIN_CLAIM => {
                if !self_signed {
                    // A claim is a device speaking about itself. Signed by
                    // anyone else it would be a member assigning origins to
                    // other people's keys.
                    r.refused.insert(id, Refusal::OriginDoesNotMatchKey);
                    continue;
                }
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
    fn a_pairing_admits_twice_and_the_older_device_stays_older() {
        // Pairing writes two admissions: the offerer admits the newcomer, then
        // the newcomer admits the offerer back. Recording the latest would
        // make the desktop that has been here all along look like the device
        // paired most recently, and §2.4.1's default notifier would pick it.
        let mut desk = testing::Origin::new(0);
        let mut phone = testing::Origin::new(1);
        let root = desk.roster_root();
        let admit_phone = desk.admits(&phone);
        phone.observe(&admit_phone, 2_000_000_000_000);
        let admit_desk = phone.admits(&desk);
        assert!(
            admit_desk.header.hlc > admit_phone.header.hlc,
            "a precondition"
        );

        let r = build(&[root.clone(), admit_phone.clone(), admit_desk.clone()]);
        assert_eq!(r.admitted.get(&desk.pubkey()), Some(&root.header.hlc));
        assert_eq!(
            r.admitted.get(&phone.pubkey()),
            Some(&admit_phone.header.hlc)
        );
        assert!(r.admitted[&phone.pubkey()] > r.admitted[&desk.pubkey()]);

        // And the same whatever order they arrive in.
        assert_eq!(build(&[admit_desk, admit_phone, root]).admitted, r.admitted);
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
        //
        // A root whose body names a different incarnation is not self-signed,
        // so it is not a root: it reads as an admission, and no one may admit
        // anything into an empty roster. That refusal is the re-evaluable one
        // on purpose — an admission genuinely can arrive before its admitter's
        // own, and calling it permanently malformed would strand it.
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
        assert_eq!(r.refused.get(&id), Some(&Refusal::AdmitterIsNotAMember));
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
