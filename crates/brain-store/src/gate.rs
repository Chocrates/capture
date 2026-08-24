//! The one door events come in through.
//!
//! # Why ingest is not a function on the log
//!
//! Verifying an event means finding the key its origin authors under, and that
//! mapping is established by other events. The log alone cannot answer it; the
//! roster can, and the roster is folded from the log. So ingest needs two
//! stores at once, and the ordering between them is load-bearing enough to
//! deserve a type rather than a convention:
//!
//! 1. The roster is built from the log **first**.
//! 2. Until it exists, nothing is accepted.
//! 3. Every membership event that lands rebuilds it, and anything previously
//!    refused for want of a key is reconsidered.
//!
//! Step 2 is the one that looks skippable. Accepting events while the roster is
//! unavailable and verifying them afterwards is not a weaker form of the same
//! guarantee — it is none, because by the time the check runs the events have
//! already taken positions that the genuine events for those positions can
//! never take back.
//!
//! # Refused, set aside, or accepted
//!
//! Three outcomes, and the difference between the first two is the difference
//! between a security signal and a version-skew signal:
//!
//! - **Refused** — not authenticated. No position, no head movement, kept only
//!   for forensics. Includes "no key is known for this origin", which is the
//!   one refusal that can stop being true, and so is the one whose bytes are
//!   kept for reconsideration.
//! - **Set aside** — authentic, and therefore genuinely entitled to its
//!   position, but unusable here: a second roster root, or a position already
//!   occupied by a different event. It keeps its place and is looked at again
//!   whenever membership changes.
//! - **Accepted** — in the log.

use brain_core::{
    Dev, Incarnation,
    body::OriginClaimBody,
    envelope::{Envelope, Rejection},
    event::Kind,
    roster::{self, Roster},
};

use crate::{
    Result,
    log::{Ingested, Log},
    ops::Ops,
    projection::Store,
};

/// What became of an event offered for ingest.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// It is in the log.
    Took(Ingested),
    /// Nothing was decided, because membership is not yet known. The event was
    /// not stored, not refused, and not counted against anything — offering it
    /// again once the roster exists is the intended response.
    Paused,
    /// Authentic, entitled to its position, and unusable for now.
    SetAside(&'static str),
    /// Not authenticated.
    Refused(Rejection),
}

/// Ingest, holding the stores it needs together.
pub struct Gate<'a> {
    log: &'a mut Log,
    state: &'a mut Store,
    ops: &'a Ops,
    roster: Roster,
}

impl<'a> Gate<'a> {
    /// Build the roster from the log, then open for business.
    ///
    /// The roster is constructed here rather than read from `state.db` because
    /// the projection may be absent, stale, or mid-rebuild, and ingest that
    /// trusts a stale roster admits events under keys that were retired. The
    /// log is the only thing that is never any of those.
    pub fn open(log: &'a mut Log, state: &'a mut Store, ops: &'a Ops) -> Result<Self> {
        let r = roster::build(&log.all()?);
        state.write_roster(&r)?;
        Ok(Gate {
            log,
            state,
            ops,
            roster: r,
        })
    }

    /// Whether anything can be accepted at all.
    pub fn is_open(&self) -> bool {
        self.roster.is_bootstrapped()
    }

    /// Offer an event.
    pub fn offer(&mut self, e: &Envelope, at: u64) -> Result<Outcome> {
        let id = e.header.id();

        // Membership events carry the key they are to be checked against, so
        // they are the only events that can be verified before the roster
        // exists — and they have to be, or the log's first event could never be
        // admitted and the rule would exclude its own starting condition.
        if is_membership_shaped(e) {
            return self.admit_membership(e, at);
        }
        if !self.roster.is_bootstrapped() {
            return Ok(Outcome::Paused);
        }

        let Some(key) = self.roster.key_for(&e.header.dev).copied() else {
            // Possibly premature rather than hostile: the claim binding this
            // origin to a key may not have arrived. Kept with its bytes so the
            // arrival of that claim can reopen the question.
            self.refuse(e, Rejection::UnknownOrigin, at, true)?;
            return Ok(Outcome::Refused(Rejection::UnknownOrigin));
        };

        let took = self.log.ingest(e, &key)?;
        match took {
            Ingested::Rejected(r) => {
                // Authentication failed under the origin's own key. No later
                // event can change that, so the bytes are not kept for retry.
                self.refuse(e, r, at, false)?;
                Ok(Outcome::Refused(r))
            }
            Ingested::Collision => {
                self.state
                    .quarantine(&id, &e.header.dev, e.header.seq, "collision", at)?;
                Ok(Outcome::SetAside("collision"))
            }
            other => {
                self.ops.forget(&id.0)?;
                self.state.release(&id)?;
                Ok(Outcome::Took(other))
            }
        }
    }

    /// Rebuild membership and look again at everything refused for want of it.
    ///
    /// Called on every membership arrival and on every rebuild. Both matter: a
    /// claim can arrive after the events it authorises, and a rebuild is the
    /// only thing that reconsiders refusals recorded by an older build.
    pub fn reconsider(&mut self) -> Result<usize> {
        self.roster = roster::build(&self.log.all()?);
        self.state.write_roster(&self.roster)?;

        let mut admitted = 0;
        for (id, raw) in self.ops.retryable("UnknownOrigin")? {
            let Ok(e) = minicbor::decode::<Envelope>(&raw) else {
                continue;
            };
            let Some(key) = self.roster.key_for(&e.header.dev).copied() else {
                continue;
            };
            match self.log.ingest(&e, &key)? {
                Ingested::Rejected(_) | Ingested::Collision => {}
                _ => {
                    self.ops.forget(&id)?;
                    admitted += 1;
                }
            }
        }

        // Anything set aside for want of membership is reconsidered too: the
        // roster just changed, which is the only thing that could have made it
        // usable.
        for (id, reason) in self.state.quarantined()? {
            if reason == "not_a_member" && !self.roster.refused.contains_key(&id) {
                self.state.release(&id)?;
            }
        }

        Ok(admitted)
    }

    /// The roster as it currently stands.
    pub fn roster(&self) -> &Roster {
        &self.roster
    }

    /// Take in a membership event, checked against the key it names.
    ///
    /// The key comes out of the event's own body, which sounds like accepting
    /// whatever an event claims about itself. It is bounded by one check: the
    /// origin must be the one that key and incarnation *produce*. An origin is
    /// `H(pubkey ‖ incarnation)`, so passing that check means the author holds
    /// the key their own origin is named after — and therefore that the event
    /// can claim no position but its own. It cannot squat another device's
    /// sequence, which is the harm the rule exists to prevent.
    ///
    /// What the check does **not** establish is membership. That is decided by
    /// the fold afterwards: a second self-signed admission passes here, keeps
    /// its position, and confers nothing.
    fn admit_membership(&mut self, e: &Envelope, at: u64) -> Result<Outcome> {
        let Some(claim) = declared(e) else {
            self.refuse(e, Rejection::UnknownOrigin, at, false)?;
            return Ok(Outcome::Refused(Rejection::UnknownOrigin));
        };
        let key = claim.pubkey;
        if Dev::derive(&key, &Incarnation(claim.incarnation)) != e.header.dev {
            self.refuse(e, Rejection::UnknownOrigin, at, false)?;
            return Ok(Outcome::Refused(Rejection::UnknownOrigin));
        }

        // A claim to a *new origin under an existing key* is only checkable
        // against a key already on the roster. Arriving before the admission it
        // depends on is normal rather than suspect, so it is kept for retry.
        if e.header.kind == Kind::ORIGIN_CLAIM && !self.roster.keys.contains(&key) {
            self.refuse(e, Rejection::UnknownOrigin, at, true)?;
            return Ok(Outcome::Refused(Rejection::UnknownOrigin));
        }

        let took = self.log.ingest(e, &key)?;
        if let Ingested::Rejected(r) = took {
            self.refuse(e, r, at, false)?;
            return Ok(Outcome::Refused(r));
        }
        self.reconsider()?;
        if self.roster.key_for(&e.header.dev).is_none() {
            // Stored, because it is authentic and holds its position, but it
            // conferred nothing — a root that lost the tiebreak to another.
            self.state.quarantine(
                &e.header.id(),
                &e.header.dev,
                e.header.seq,
                "not_a_member",
                at,
            )?;
            return Ok(Outcome::SetAside("not_a_member"));
        }
        Ok(Outcome::Took(took))
    }

    fn refuse(&self, e: &Envelope, r: Rejection, at: u64, keep_bytes: bool) -> Result<()> {
        let raw = if keep_bytes {
            let mut b = Vec::new();
            minicbor::encode(e, &mut b).expect("encoding into Vec is infallible");
            Some(b)
        } else {
            None
        };
        self.ops.reject(
            &e.header.id().0,
            Some(&dev_hex(&e.header.dev)),
            Some(e.header.seq),
            &format!("{r:?}"),
            at,
            raw.as_deref(),
        )
    }
}

/// A membership event in the only shape that can be self-checked: the first
/// event of a fresh origin, with no predecessor.
///
/// The shape matters. Without it, a member could append a "membership" event
/// deep inside its own sequence and have it verified against a key of its
/// choosing rather than the one its origin is bound to.
fn is_membership_shaped(e: &Envelope) -> bool {
    (e.header.kind == Kind::DEVICE_ADD || e.header.kind == Kind::ORIGIN_CLAIM)
        && e.header.seq == 1
        && e.header.chain.is_none()
}

fn declared(e: &Envelope) -> Option<OriginClaimBody> {
    minicbor::decode::<OriginClaimBody>(e.body.as_deref()?).ok()
}

fn dev_hex(d: &Dev) -> String {
    d.0.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::testing;
    use std::path::PathBuf;

    struct Bench {
        dir: PathBuf,
        log: Log,
        state: Store,
        ops: Ops,
    }

    fn bench(name: &str) -> Bench {
        let dir = std::env::temp_dir().join(format!("brain-gate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Bench {
            log: Log::open(&dir.join("events.db")).unwrap(),
            state: Store::open(&dir.join("state.db")).unwrap(),
            ops: Ops::open(&dir.join("ops.db")).unwrap(),
            dir,
        }
    }

    impl Drop for Bench {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.dir).ok();
        }
    }

    #[test]
    fn nothing_is_accepted_before_a_roster_exists() {
        let mut b = bench("paused");
        let s = testing::mixed(1, 1, 2, 4);
        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops).unwrap();
        assert!(!g.is_open());
        assert_eq!(g.offer(&s.events[0], 100).unwrap(), Outcome::Paused);
        assert_eq!(
            g.log.count().unwrap(),
            0,
            "a paused offer must leave nothing behind"
        );
        assert_eq!(
            b.ops.rejected_total().unwrap(),
            0,
            "and must not be counted as a refusal either"
        );
    }

    #[test]
    fn a_root_bootstraps_the_roster_it_needs() {
        let mut b = bench("bootstrap");
        let mut d = testing::Origin::new(0);
        let root = d.roster_root();

        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops).unwrap();
        assert!(matches!(g.offer(&root, 100).unwrap(), Outcome::Took(_)));
        assert!(g.is_open());
        assert_eq!(g.roster().key_for(&d.dev()), Some(&d.pubkey()));
    }

    #[test]
    fn an_origin_with_no_key_is_refused_but_kept() {
        let mut b = bench("no-key");
        let mut member = testing::Origin::new(0);
        let root = member.roster_root();
        let mut stranger = testing::Origin::new(4);
        let e = stranger.author(
            brain_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "from nowhere"),
            1_700_000_000_000,
        );

        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops).unwrap();
        g.offer(&root, 100).unwrap();
        assert_eq!(
            g.offer(&e, 101).unwrap(),
            Outcome::Refused(Rejection::UnknownOrigin)
        );
        assert_eq!(
            b.ops.retryable("UnknownOrigin").unwrap().len(),
            1,
            "the bytes must survive, or the origin is permanently unusable"
        );
    }

    #[test]
    fn a_late_claim_admits_the_events_that_were_waiting_on_it() {
        // The whole reason refusals for want of a key keep their bytes: a
        // restore's events routinely arrive before the claim explaining them.
        let mut b = bench("late-claim");
        let mut first = testing::Origin::new(0);
        let root = first.roster_root();
        let mut restored = first.restored([9u8; 16]);
        let claim = restored.origin_claim();
        let e = restored.author(
            brain_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "after the restore"),
            1_700_000_100_000,
        );

        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops).unwrap();
        g.offer(&root, 100).unwrap();
        // Capture first, claim second — the order that would lose data.
        assert_eq!(
            g.offer(&e, 101).unwrap(),
            Outcome::Refused(Rejection::UnknownOrigin)
        );
        assert!(matches!(g.offer(&claim, 102).unwrap(), Outcome::Took(_)));

        assert_eq!(
            g.roster().key_for(&restored.dev()),
            Some(&first.pubkey()),
            "the claim binds the new origin"
        );
        let heads = g.log.heads().unwrap();
        assert_eq!(
            heads.get(&dev_hex(&restored.dev())),
            Some(&2),
            "and the capture that was waiting on it is now in the log"
        );
        assert_eq!(
            b.ops.retryable("UnknownOrigin").unwrap().len(),
            0,
            "the refusal is dropped once the event is really held"
        );
    }

    #[test]
    fn a_forged_signature_is_refused_without_its_bytes() {
        // A refusal that can never stop being true. Keeping the bytes for retry
        // would mean re-verifying the forensic pile on every membership event.
        let mut b = bench("forged");
        let mut d = testing::Origin::new(0);
        let root = d.roster_root();
        let mut e = d.author(
            brain_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "tampered"),
            1_700_000_000_000,
        );
        e.sig[0] ^= 0xFF;

        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops).unwrap();
        g.offer(&root, 100).unwrap();
        assert_eq!(
            g.offer(&e, 101).unwrap(),
            Outcome::Refused(Rejection::BadSignature)
        );
        assert_eq!(g.log.count().unwrap(), 1, "the root, and nothing else");
        assert_eq!(b.ops.retryable("UnknownOrigin").unwrap().len(), 0);
        assert_eq!(b.ops.rejected_total().unwrap(), 1);
    }

    #[test]
    fn a_second_root_keeps_its_position_without_becoming_a_member() {
        let mut b = bench("second-root");
        let mut a = testing::Origin::new(0);
        let mut c = testing::Origin::new(1);
        let first = a.roster_root();
        let second = c.roster_root();

        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops).unwrap();
        g.offer(&first, 100).unwrap();
        let out = g.offer(&second, 101).unwrap();

        // Whichever way the timestamp tiebreak fell, exactly one key is a
        // member and the other root is stored rather than discarded.
        assert_eq!(g.roster().keys.len(), 1);
        assert_eq!(g.log.count().unwrap(), 2, "both hold their positions");
        assert!(
            matches!(out, Outcome::Took(_) | Outcome::SetAside(_)),
            "a second root is authentic, so it is never refused: {out:?}"
        );
    }

    #[test]
    fn a_reopened_gate_rebuilds_membership_from_the_log() {
        // The projection is disposable; membership must not be lost with it.
        let mut b = bench("reopen");
        let mut d = testing::Origin::new(0);
        let root = d.roster_root();
        {
            let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops).unwrap();
            g.offer(&root, 100).unwrap();
        }
        let g = Gate::open(&mut b.log, &mut b.state, &b.ops).unwrap();
        assert!(g.is_open());
        assert_eq!(g.roster().key_for(&d.dev()), Some(&d.pubkey()));
    }

    #[test]
    fn a_capture_replayed_twice_changes_nothing() {
        let mut b = bench("replay");
        let mut d = testing::Origin::new(0);
        let root = d.roster_root();
        let e = d.author(
            brain_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "once"),
            1_700_000_000_000,
        );

        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops).unwrap();
        g.offer(&root, 100).unwrap();
        assert_eq!(g.offer(&e, 101).unwrap(), Outcome::Took(Ingested::Accepted));
        assert_eq!(
            g.offer(&e, 102).unwrap(),
            Outcome::Took(Ingested::Duplicate)
        );
        assert_eq!(g.log.count().unwrap(), 2);
    }
}
