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

use capture_core::{
    Dev, Incarnation,
    body::OriginClaimBody,
    envelope::{Envelope, Rejection},
    event::Kind,
    hlc::Hlc,
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
    /// This device's own signing key, for the self-retirement guard.
    own: [u8; 32],
    /// The highest stamp on anything that authenticated here, for the caller to
    /// adopt into its clock.
    ///
    /// Collected rather than applied: the gate holds the log and the projection
    /// and not the clock, which belongs to the session that persists it. See
    /// [`Gate::highest_stamp`].
    seen: Option<Hlc>,
}

impl<'a> Gate<'a> {
    /// Build the roster from the log, then open for business.
    ///
    /// The roster is constructed here rather than read from `state.db` because
    /// the projection may be absent, stale, or mid-rebuild, and ingest that
    /// trusts a stale roster admits events under keys that were retired. The
    /// log is the only thing that is never any of those.
    /// `own` is this device's own signing key.
    ///
    /// Required rather than optional: what it enables is the refusal to accept
    /// a retirement of *this* device that this device did not issue, and an
    /// `Option` would make forgetting it a silent loss of that guard.
    pub fn open(
        log: &'a mut Log,
        state: &'a mut Store,
        ops: &'a Ops,
        own: [u8; 32],
    ) -> Result<Self> {
        let mut r = roster::build(&log.all()?);
        // The first of §5.3's three lockout guards. Applied here rather than
        // left to callers because a guard nobody remembers to apply is one the
        // design claims and the code does not have — and this one is inert
        // right up until a malfunctioning peer retires every other device.
        r.honour_self(&own);
        state.write_roster(&r)?;
        Ok(Gate {
            log,
            state,
            ops,
            roster: r,
            own,
            seen: None,
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

        // Past the head a retirement named. The event is authentic — the
        // signature checks out under the origin's own key — so it is set
        // aside rather than rejected: rejection would leave a hole in the
        // origin's numbering that anti-entropy re-requests for ever.
        //
        // The head is enforceable where a timestamp would not be. A retired
        // device chooses its own `hlc` and can backdate, and there is no
        // causal ordering between a retirement and the events of the device
        // being retired — but the per-origin chain makes "beyond this event"
        // provable whatever the clock says.
        if let Some((head, _)) = self.roster.head_of(&e.header.dev)
            && e.header.seq > head
        {
            // Stored in its position, and marked unusable.
            //
            // It used to be kept out of the log entirely, which left `heads`
            // for this origin stopped at the retirement head — and a version
            // vector cannot describe a hole. Every round afterwards asked every
            // peer for the tail above it, was sent the whole thing, set it
            // aside again and asked again, for ever. The spec calls for
            // set-aside rather than refusal to avoid exactly that, and keeping
            // the event out of the log was the refusal in all but name.
            //
            // The position being occupied is what ends the loop. The `aside`
            // column is what keeps the event out of every fold and every roster
            // build — which matters most on the roster: a retired device's
            // admission, folded, is a retired device choosing a new member.
            //
            // It also makes the remedy local. A retirement reissued with a
            // later head re-admits the events below it, and `release` used to
            // delete a row holding an id, an origin and a sequence — a record
            // of an event rather than the event. Recovering them depended on a
            // peer still being reachable and still offering them. Now the
            // bytes are here.
            let took = self.log.ingest_aside(e, &key, "post_retire")?;
            if !matches!(took, Ingested::Rejected(_)) {
                self.witness(e);
            }
            self.state
                .quarantine(&id, &e.header.dev, e.header.seq, "post_retire", at)?;
            return Ok(Outcome::SetAside("post_retire"));
        }

        let took = self.log.ingest(e, &key)?;
        if !matches!(took, Ingested::Rejected(_)) {
            self.witness(e);
        }
        match took {
            Ingested::Rejected(r) => {
                // Authentication failed under the origin's own key. No later
                // event can change that, so the bytes are not kept for retry.
                self.refuse(e, r, at, false)?;
                Ok(Outcome::Refused(r))
            }
            Ingested::Collision => {
                // The bytes are kept, and this is the one refusal where that is
                // the whole point.
                //
                // `log.rs` says plainly that treating a collision as benign is
                // silent, permanent, two-way data loss. The quarantine row
                // records that it happened — id, origin, sequence — and records
                // none of what arrived, while `ingest` returns success to the
                // peer, which then counts the event as delivered and never
                // offers it again. Anti-entropy will not ask either: the local
                // head is already past that sequence. So the event existed on
                // exactly one other device, was handed over, was thrown away,
                // and both sides believe it was filed.
                //
                // Kept in `ops` rather than in the projection, because the
                // projection is a derived artifact that is deleted and rebuilt
                // from the log — and these bytes are precisely the ones that are
                // *not* in the log. A rebuild would be the second time the same
                // event was discarded.
                //
                // Not retried: `reconsider` asks for specific reasons, and no
                // arrival of a key or a claim makes a taken position free. This
                // is a copy for a person to recover by hand, which is the only
                // remedy a fork admits.
                let mut raw = Vec::new();
                minicbor::encode(e, &mut raw).expect("encoding into Vec is infallible");
                self.ops.reject(
                    &id.0,
                    Some(&dev_hex(&e.header.dev)),
                    Some(e.header.seq),
                    "collision",
                    at,
                    Some(&raw),
                )?;
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
        self.roster.honour_self(&self.own);
        self.state.write_roster(&self.roster)?;

        let mut admitted = 0;
        // A retirement reissued naming a later head re-admits exactly the
        // events below it — an explicit decision about what to keep, rather
        // than a silent race. Nothing else would reconsider them: they are
        // authentic, so no arrival of a key or a claim changes anything about
        // them.
        //
        // Released against the *current* head rather than unconditionally.
        // Releasing everything would re-admit the events the retirement exists
        // to exclude, which is the whole remedy undone by a housekeeping loop.
        for (id, dev, seq) in self.state.quarantined_at("post_retire")? {
            let still_excluded = self
                .roster
                .origins
                .keys()
                .find(|d| crate::projection::dev_hex(d) == dev)
                .and_then(|d| self.roster.head_of(d))
                .is_some_and(|(head, _)| seq > head);
            if !still_excluded {
                // The record, and the event itself.
                //
                // Releasing the quarantine row clears an id, an origin and a
                // sequence — the thing the surface was reporting, which is not
                // the event. The event is in the log in its own position with
                // the mark that keeps it out of every fold, and clearing that
                // mark is what carrying out the decision means. Nothing is
                // fetched and no head moves.
                //
                // Before these were stored at all, this depended on a peer
                // offering them again: the remedy for "keep those after all"
                // required the retired device's neighbour to still be
                // reachable and still be holding them.
                self.state.release(&id)?;
                if self.log.release_aside(&id)? {
                    admitted += 1;
                }
            }
        }
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
                    self.witness(&e);
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
    /// The highest stamp carried by anything that authenticated through this
    /// gate, or `None` if nothing did.
    ///
    /// # Why this is not applied here
    ///
    /// Adoption is the one thing that keeps the HLC causal: `hlc.rs` is explicit
    /// that a device must advance its clock to at least any value it has seen,
    /// and that failing to means its own later edits lose every last-writer-wins
    /// race for the duration of the skew. Nothing in the ingest path did it —
    /// `observe` had no caller outside the test harness, so in a running system
    /// two devices' conflicts were decided by raw clock difference rather than by
    /// causality, and a fold could reach an edit before the version it replaces.
    ///
    /// It is collected here and applied by the caller because the clock lives in
    /// the session, which is what persists it. A gate holding its own copy would
    /// be a second clock.
    pub fn highest_stamp(&self) -> Option<Hlc> {
        self.seen
    }

    /// Note the stamp of an event whose signature verified.
    ///
    /// Authenticity is the bar, not acceptance. An event set aside for a
    /// collision verified under its origin's own key — it was genuinely authored
    /// at that time by a device holding that key — and its stamp is part of what
    /// this device has now seen. Ignoring it would leave the clock behind exactly
    /// when a conflict is being resolved.
    ///
    /// An event refused for a bad signature is not witnessed. Witnessing one
    /// would let an unauthenticated stamp drag this device's clock — an
    /// unsigned event claiming the year 2400 would distort every comparison
    /// until the wall clock caught up, which is the one thing adoption must not
    /// allow.
    ///
    /// A `post_retire` event *is* witnessed, and that changed when it started
    /// being stored: it now goes through `ingest_aside`, which verifies it
    /// under the origin's own key before anything else happens. It was authored
    /// by a device holding that key, at that time, and this device now holds it
    /// — all of which is true whether or not the roster will let it be used.
    /// What is withheld from it is the fold, not the fact that it exists.
    fn witness(&mut self, e: &Envelope) {
        let stamp = e.header.hlc;
        if self.seen.is_none_or(|s| stamp > s) {
            self.seen = Some(stamp);
        }
    }

    /// Fold arrived events into the projection.
    ///
    /// The gate puts events in the log and stops there, because the log is the
    /// archive and everything else is derived. Something still has to derive
    /// it, and for events that arrived from a peer that something is here.
    ///
    /// Incremental rather than a rebuild: a restore touches every note at once
    /// and earns a rebuild, while a sync round touches whatever a peer wrote
    /// since last time — usually a handful.
    pub fn fold(&mut self, arriving: &[Envelope], now: i64) -> Result<usize> {
        let n = crate::incremental::apply(self.state, self.log, arriving, now)?;
        // The sync path, and the one where falling behind was invisible: a
        // round that ingests and then loses the connection before folding
        // leaves the log ahead of the projection with nothing to notice it.
        crate::projection::note_folded(self.state, arriving)?;
        Ok(n)
    }

    /// The log this gate guards, for reads.
    ///
    /// Read-only on purpose: every write goes through `offer`, which is the
    /// point of the gate existing.
    pub fn log(&self) -> &Log {
        self.log
    }

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
        self.witness(e);
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
    use capture_core::testing;
    use std::path::PathBuf;

    struct Bench {
        dir: PathBuf,
        log: Log,
        state: Store,
        ops: Ops,
    }

    /// A key that is nobody's, so the self-retirement guard never fires and
    /// these tests observe the roster as every *other* device sees it.
    const OWN: [u8; 32] = [0xEE; 32];

    fn bench(name: &str) -> Bench {
        let dir = std::env::temp_dir().join(format!("capture-gate-{name}-{}", std::process::id()));
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
        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, OWN).unwrap();
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

        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, OWN).unwrap();
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
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "from nowhere"),
            1_700_000_000_000,
        );

        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, OWN).unwrap();
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
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "after the restore"),
            1_700_000_100_000,
        );

        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, OWN).unwrap();
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
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "tampered"),
            1_700_000_000_000,
        );
        e.sig[0] ^= 0xFF;

        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, OWN).unwrap();
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

        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, OWN).unwrap();
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
            let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, OWN).unwrap();
            g.offer(&root, 100).unwrap();
        }
        let g = Gate::open(&mut b.log, &mut b.state, &b.ops, OWN).unwrap();
        assert!(g.is_open());
        assert_eq!(g.roster().key_for(&d.dev()), Some(&d.pubkey()));
    }

    #[test]
    fn a_capture_replayed_twice_changes_nothing() {
        let mut b = bench("replay");
        let mut d = testing::Origin::new(0);
        let root = d.roster_root();
        let e = d.author(
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "once"),
            1_700_000_000_000,
        );

        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, OWN).unwrap();
        g.offer(&root, 100).unwrap();
        assert_eq!(g.offer(&e, 101).unwrap(), Outcome::Took(Ingested::Accepted));
        assert_eq!(
            g.offer(&e, 102).unwrap(),
            Outcome::Took(Ingested::Duplicate)
        );
        assert_eq!(g.log.count().unwrap(), 2);
    }
    #[test]
    fn a_retired_devices_later_events_are_set_aside() {
        // The event is authentic — its signature checks out under the origin's
        // own key — so it is set aside rather than rejected. Rejecting it would
        // leave a hole in the origin's numbering, and anti-entropy would
        // re-request that range from every peer for ever.
        let mut b = bench("post-retire");
        let mut laptop = testing::Origin::new(0);
        let mut phone = testing::Origin::new(2);

        let root = laptop.roster_root();
        let add_phone = laptop.admits(&phone);
        phone.observe(&add_phone, 1_700_000_001_000);

        let kept = phone.author(
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "before the phone was lost"),
            1_700_000_002_000,
        );
        let after = phone.author(
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(2, "written by whoever has it now"),
            1_700_000_003_000,
        );
        let retire = laptop.retires(&phone, &[(phone.dev(), kept.header.seq, kept.header.id())]);

        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, OWN).unwrap();
        g.offer(&root, 100).unwrap();
        g.offer(&add_phone, 101).unwrap();
        g.offer(&kept, 102).unwrap();
        g.offer(&retire, 103).unwrap();
        g.reconsider().unwrap();

        assert_eq!(
            g.offer(&after, 104).unwrap(),
            Outcome::SetAside("post_retire")
        );
    }

    #[test]
    fn a_backdated_event_from_a_retired_device_is_still_excluded() {
        // The rule a timestamp cannot express. A retired device chooses its own
        // hlc and can claim any moment it likes; the head is enforceable
        // because the per-origin chain makes "beyond this event" provable
        // whatever the clock says.
        let mut b = bench("post-retire-backdated");
        let mut laptop = testing::Origin::new(0);
        let mut phone = testing::Origin::new(2);

        let root = laptop.roster_root();
        let add_phone = laptop.admits(&phone);
        phone.observe(&add_phone, 1_700_000_001_000);
        let kept = phone.author(
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "legitimate"),
            1_700_000_002_000,
        );
        // Authored *after* the retirement, stamped long before it.
        let backdated = phone.author(
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(2, "claims to predate the retirement"),
            1_600_000_000_000,
        );
        let retire = laptop.retires(&phone, &[(phone.dev(), kept.header.seq, kept.header.id())]);

        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, OWN).unwrap();
        for e in [&root, &add_phone, &kept, &retire] {
            g.offer(e, 100).unwrap();
        }
        g.reconsider().unwrap();

        assert_eq!(
            g.offer(&backdated, 105).unwrap(),
            Outcome::SetAside("post_retire"),
            "an event was admitted because it claimed an earlier timestamp"
        );
    }

    #[test]
    fn a_reissued_retirement_readmits_what_it_now_covers() {
        // The retired device had already propagated events past the named
        // head. Reissuing with a later head is the explicit decision about
        // what to keep, and the events below it come back rather than staying
        // set aside for ever.
        let mut b = bench("post-retire-reissue");
        let mut laptop = testing::Origin::new(0);
        let mut phone = testing::Origin::new(2);

        let root = laptop.roster_root();
        let add_phone = laptop.admits(&phone);
        phone.observe(&add_phone, 1_700_000_001_000);
        let first = phone.author(
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "one"),
            1_700_000_002_000,
        );
        let second = phone.author(
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(2, "two"),
            1_700_000_003_000,
        );
        let narrow = laptop.retires(
            &phone,
            &[(phone.dev(), first.header.seq, first.header.id())],
        );

        let mut g = Gate::open(&mut b.log, &mut b.state, &b.ops, OWN).unwrap();
        for e in [&root, &add_phone, &first, &narrow] {
            g.offer(e, 100).unwrap();
        }
        g.reconsider().unwrap();
        assert_eq!(
            g.offer(&second, 105).unwrap(),
            Outcome::SetAside("post_retire")
        );

        // Reissued, naming both the later head and the retirement it
        // replaces. Without naming it, lowest-head-wins would keep the
        // narrower claim and a reissue could never widen anything.
        let wider = laptop.retires_superseding(
            &phone,
            &[(phone.dev(), second.header.seq, second.header.id())],
            Some(narrow.header.id()),
        );
        g.offer(&wider, 106).unwrap();
        g.reconsider().unwrap();

        assert!(
            !b.state
                .quarantined_at("post_retire")
                .unwrap()
                .iter()
                .any(|(id, _, _)| *id == second.header.id()),
            "the reissued retirement did not release the event it now covers"
        );
    }
}
