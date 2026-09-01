//! Holding a store open for writing, and authoring into it.
//!
//! Lives here rather than in a binary because there are two of them — the
//! command line and the overlay — and they must write identically. A capture
//! made by the chord and a capture made by `capture +` differ in how the text
//! arrives and in nothing else; two implementations of that would drift, and
//! the drift would be in the one path this system exists to make reliable.
//!
//! # One place where impurity enters
//!
//! Below this, nothing reads a clock or draws entropy: `capture-core` cannot name
//! either, and `capture-store` takes them as arguments. Here is where the values
//! are actually produced — the wall clock, the capture nonce, the time zone —
//! and there is exactly one function that does it, so "what did this event
//! observe about the world" has a single answer.
//!
//! # The sequence number comes from the log, not the watermark
//!
//! The watermark records how far this device has written, and is deliberately
//! written *after* the event commits. So it can lag: a crash between the two
//! leaves it one behind. Reading it to choose the next sequence number would
//! then reuse a number that is already taken.
//!
//! The log knows. `heads` is updated inside the same transaction as the row, so
//! it is never ahead of what exists and never behind it. The watermark's job is
//! to detect a *restore* — a store that has moved backwards relative to what
//! this device remembers writing — and that job needs it to be allowed to lag.

use crate::{
    Error, Identity, Result, StorePaths, ambient, drain, entropy, incremental,
    lock::{self, Writer},
    log::Log,
    ops::Ops,
    projection::{self, Store},
    spool, store,
};
use capture_core::{
    body::CaptureBody,
    envelope::Envelope,
    event::{ENVELOPE_VERSION, Header, Kind},
    hlc::HlcState,
    id::BodyHash,
};
use ed25519_dalek::Signer;

/// An open store this process is entitled to write to.
pub struct Session {
    pub paths: StorePaths,
    pub identity: Identity,
    pub log: Log,
    pub state: Store,
    clock: HlcState,
    /// What the store looked like when it was opened.
    pub startup: crate::startup::State,
    /// Held for as long as the session is. Dropping it releases the store.
    _writer: Writer,
}

fn alloc_vec(id: capture_core::EventId) -> Vec<capture_core::EventId> {
    vec![id]
}

/// What a pairing learned about the device being admitted.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Admitted {
    /// The signing key membership is keyed by (§5.3).
    pub pubkey: [u8; 32],
    /// The incarnation whose origin is admitted with it.
    pub incarnation: [u8; 16],
    /// The X25519 key a session is opened against.
    pub noise: [u8; 32],
}

impl Session {
    /// Open a store for writing.
    ///
    /// Fails rather than waiting when another process holds the store. In a
    /// finished system a non-writer hands its work to the writer over a socket;
    /// until that exists, saying plainly who has it is better than blocking on
    /// something that may be a long-running overlay rather than a passing
    /// command.
    /// Open a store for writing a capture, and nothing else.
    ///
    /// Identical to [`Session::open`] except that it does not read anything.
    ///
    /// The distinction is the whole of §3.1's promise. Parsing is a sweep over
    /// what is already stored, and a sweep can touch two hundred notes; putting
    /// it on the path a capture takes would mean every `capture +` waited for it,
    /// which is the one thing the design will not spend. A capture is the one
    /// operation that gets a store to itself.
    pub fn open_for_capture(paths: StorePaths) -> Result<Session> {
        Session::start(paths, false)
    }

    pub fn open(paths: StorePaths) -> Result<Session> {
        Session::start(paths, true)
    }

    fn start(paths: StorePaths, catch_up: bool) -> Result<Session> {
        let identity = store::open(&paths)?;
        let lock_path = paths.root.join("writer.lock");
        let Some(writer) = Writer::acquire(&lock_path, "cli")? else {
            let who = lock::holder(&lock_path).unwrap_or_else(|| "another process".into());
            return Err(Error::Usage(format!("the store is being written by {who}")));
        };

        let log = Log::open(&paths.events)?;
        let generation = Ops::open(&paths.ops)?.generation()?;
        let projection_store = Store::open(&projection::generation_path(&paths.state, generation))?;

        // Resume the clock. Not doing so is how a device issues a timestamp it
        // has already used: a fresh clock starts at zero, and the first event
        // after a restart would sort before everything written before it.
        let w = identity.watermark();
        let clock = HlcState::restore(w.hlc_wall, w.hlc_counter);

        // What the two records say about each other. Directional: the event
        // commits before the watermark is written, so the watermark lagging is
        // an ordinary crash and the watermark leading means the store has lost
        // events this device already handed positions to.
        let log_head = log
            .heads()?
            .get(&dev_hex(&identity.dev()))
            .copied()
            .unwrap_or(0);
        let state = crate::startup::detect(true, log.count()? > 0, w.highest_seq, log_head);
        if let Some(m) = state.message() {
            tracing::warn!("{m}");
        }

        let mut s = Session {
            paths,
            identity,
            log,
            state: projection_store,
            clock,
            startup: state,
            _writer: writer,
        };

        // A store that went backwards gets a fresh origin before anything is
        // written. The alternative is handing out sequence numbers that other
        // replicas already hold, which loses both events and reports nothing.
        if s.startup.needs_new_origin() {
            let dev = s.reincarnate()?;
            tracing::warn!(origin = ?dev.short(), "started a new origin");
            s.startup = crate::startup::State::Normal;
        }

        // Anything a previous run left halfway through destroying, finished
        // before anything else looks at the store. An interrupted purge is
        // invisible in the log — the redaction event reads the same before and
        // after — so this is the only thing that notices.
        match crate::purge::resume(
            &mut s.log,
            &mut s.state,
            &Ops::open(&s.paths.ops)?,
            now_ms(),
        ) {
            Ok(0) => {}
            Ok(n) => tracing::warn!(n, "finished purges interrupted by an earlier run"),
            Err(e) => tracing::error!(%e, "could not finish an interrupted purge"),
        }

        // A projection whose shape predates this build.
        //
        // §8.8 makes the projection derived and nothing in it authoritative,
        // which is what allows the entire migration strategy to be "throw it
        // away and compute it again" — no migration code, and none ever
        // accumulates. What was missing was anything that *noticed*:
        // `is_stale` existed and was called only by its own tests, so a schema
        // change left every existing store on the old shape until somebody
        // happened to run `capture rebuild` by hand.
        //
        // Here rather than at every open, because it costs a full fold and only
        // ever fires once per upgrade.
        if catch_up && s.state.is_stale().unwrap_or(false) {
            match s.rebuild_projection() {
                Ok(g) => tracing::info!(generation = g, "projection rebuilt for a new schema"),
                Err(e) => tracing::warn!(%e, "the projection could not be rebuilt"),
            }
        }

        // Read anything nothing has read yet, or that an older grammar read.
        //
        // Every startup except a capture's. §3.1 puts parsing after the commit
        // and the gesture must never wait for it, so the one path that must stay
        // fast opts out and every other one catches the store up.
        if catch_up {
            match crate::promote::sweep(&mut s) {
                Ok(0) => {}
                Ok(n) => tracing::info!(promoted = n, "notes read at startup"),
                Err(e) => tracing::warn!(%e, "notes could not be read"),
            }
        }

        // Deadlines that have arrived. Every process startup is the floor: no
        // daemon, no timer unit, and therefore nothing that can be silently not
        // running while redactions sit undestroyed.
        match s.drain_due() {
            Ok(d) if d.any() => {
                tracing::info!(destroyed = d.destroyed, "overdue work completed at startup")
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(%e, "due work could not be processed"),
        }

        // Startup is one of the two moments the store is known to be writable.
        // Anything the spool is holding goes in now, before whatever this
        // process was actually opened to do.
        match s.drain_spool() {
            Ok(0) => {}
            Ok(n) => tracing::info!(n, "spooled captures committed"),
            // The store is writable enough to have opened and not enough to
            // drain. Whatever this process was for may still work, and the
            // spool keeps everything until it does.
            Err(e) => tracing::warn!(%e, "spool could not be drained"),
        }
        Ok(s)
    }

    /// Store a capture, and return the note it became.
    /// Commit a prepared capture body.
    ///
    /// Takes the encoded body rather than the text because the body must be
    /// built *before* the store is opened. Opening can fail — that is much of
    /// what the spool is for — and a body that only exists after a successful
    /// open cannot be spooled when the open is what failed.
    pub fn capture(&mut self, body: &[u8]) -> Result<Captured> {
        let id = self.append_capture(body)?;

        // One of the two moments the store is known to be writable. A disk
        // recovering is invisible to a clock and obvious here.
        if let Err(e) = self.drain_spool() {
            tracing::warn!(%e, "spool could not be drained after a capture");
        }

        let slug = self
            .state
            .slug_of(&id)?
            .ok_or_else(|| Error::Integrity("a stored capture produced no note".into()))?;
        Ok(Captured { slug })
    }

    fn append_capture(&mut self, body: &[u8]) -> Result<capture_core::EventId> {
        let e = self.author(Kind::CAPTURE, None, None, body.to_vec())?;
        let id = e.header.id();
        self.commit(&[e])?;
        Ok(id)
    }

    /// Move anything waiting in the spool into the log.
    ///
    /// Event-driven, never scheduled. A timer retrying against a still-full
    /// disk is pure waste, and the moment a disk recovers is invisible to a
    /// clock — but it is exactly visible to the next append that succeeds. So
    /// this runs at startup and after each successful write, which are the two
    /// moments the store is known to be working.
    pub fn drain_spool(&mut self) -> Result<usize> {
        if !spool::in_use(&self.paths.spool)? {
            return Ok(0);
        }
        let mut authored: Vec<capture_core::envelope::Envelope> = Vec::new();
        let paths = self.paths.clone();

        // The position to write at is worked out once and then carried forward.
        // Re-reading the log inside the loop is not possible — the drain holds
        // it — and would be wrong anyway: appending in order means the next
        // position is simply the last one plus one.
        let dev = self.identity.dev();
        let mut seq = next_seq(&self.log, &dev)?;
        let mut tip = chain_tip(&self.log, &dev)?;

        let done = {
            let identity = &mut self.identity;
            let clock = &mut self.clock;
            let log = &mut self.log;
            drain::drain(&paths.spool, log, |payload| {
                let hlc = clock.issue(now_ms(), dev);
                let header = Header {
                    v: ENVELOPE_VERSION,
                    dev,
                    seq,
                    hlc,
                    kind: Kind::CAPTURE,
                    tgt: None,
                    prev: None,
                    occ: None,
                    chain: tip,
                    bhash: BodyHash::of(payload),
                };
                let sig = identity.signing().sign(&header.id().0).to_bytes();
                let e = Envelope {
                    header,
                    body: Some(payload.to_vec()),
                    sig,
                };
                // A record the log already holds is skipped by the drain
                // without being authored, so advancing here — inside the
                // closure — keeps the numbering aligned with what is actually
                // appended.
                seq += 1;
                tip = Some(e.header.id());
                authored.push(e.clone());
                Ok(e)
            })?
        };

        if !authored.is_empty() {
            incremental::apply(&mut self.state, &self.log, &authored, now_ms() as i64)?;
            if let Some(last) = authored.last() {
                let (wall, counter) = self.clock.persist();
                self.identity
                    .advance(&self.paths, last.header.seq, wall, counter)?;
            }
        }
        Ok(done.committed)
    }

    /// Mint a new origin and claim it, returning the new origin's identity.
    ///
    /// Called after a restore, before anything local is written. Reusing the
    /// old origin would mean reusing sequence numbers that peers already hold,
    /// which loses events in both directions and reports nothing.
    ///
    /// The claim is authored first and appended first, so there is no window in
    /// which events exist under an origin no key is bound to.
    pub fn reincarnate(&mut self) -> Result<capture_core::Dev> {
        self.identity.reincarnate(&self.paths)?;

        let nonce = entropy::nonce()?;
        let claim = crate::genesis::origin_claim(
            self.identity.signing(),
            &self.identity.incarnation(),
            &mut self.clock,
            now_ms(),
            nonce,
            self.identity.noise_public(),
        )?;
        self.log.append(&claim)?;

        let (wall, counter) = self.clock.persist();
        self.identity
            .advance(&self.paths, claim.header.seq, wall, counter)?;
        Ok(self.identity.dev())
    }

    /// Carry out whatever has become due.
    ///
    /// Returns what it did, so `capture status` can report when this last ran —
    /// without that, a long-dead overlay means redactions sitting undestroyed
    /// while every surface reports them complete.
    /// Throw the projection away and compute it again from the log.
    ///
    /// Written beside the current generation, with the pointer moved afterwards
    /// — never renamed over an open database, because readers keep the file
    /// they already opened and SQLite's sidecars are named from the path.
    pub fn rebuild_projection(&mut self) -> Result<u64> {
        let events = self.log.all()?;
        let ops = Ops::open(&self.paths.ops)?;
        let current = ops.generation()?;

        let next =
            crate::projection::rebuild(&self.paths.state, &events, current, now_ms() as i64)?;
        ops.set_generation(next)?;

        self.state = Store::open(&projection::generation_path(&self.paths.state, next))?;
        Ok(next)
    }

    pub fn drain_due(&mut self) -> Result<crate::due::Drained> {
        self.drain_due_at(now_ms())
    }

    /// Drain as of a given moment.
    ///
    /// The clock is a parameter for the same reason `store::create` takes one:
    /// a redaction does not execute until its cancellation window has passed,
    /// and a test that could not move time could only ever assert that the
    /// destruction was *queued* — which is indistinguishable from the content
    /// still being on disk, and is the one outcome the feature exists to
    /// prevent.
    pub fn drain_due_at(&mut self, now: u64) -> Result<crate::due::Drained> {
        let ops = Ops::open(&self.paths.ops)?;
        let mut done = crate::due::Drained::default();

        // What each origin has already destroyed, for the ceiling below.
        let history = self.state.redaction_history()?;

        for work in crate::due::due(&self.state.redact_queue()?, now) {
            match work {
                crate::due::Work::Destroy { redact, target } => {
                    // The ceiling is checked here, at the moment of
                    // destruction, rather than when the redaction was written.
                    // A redaction can arrive from another device long after it
                    // was issued, and what matters is how much is being
                    // destroyed *now* — a hundred redactions arriving together
                    // after a month offline is exactly the shape of the runaway
                    // this bounds, whatever their timestamps say.
                    if let crate::ratelimit::Verdict::Hold { already } =
                        crate::ratelimit::check(&history, 1, now)
                    {
                        tracing::warn!(
                            already,
                            "redaction held: this origin has destroyed a great deal in the last hour"
                        );
                        done.held += 1;
                        continue;
                    }

                    crate::purge::execute(
                        &mut self.log,
                        &mut self.state,
                        &ops,
                        &redact,
                        &target,
                        now,
                    )?;
                    done.destroyed += 1;
                }
                crate::due::Work::ExpireSnapshot { .. } => done.expired += 1,
            }
        }

        // Copies whose retention has elapsed. Held any longer they stop being a
        // safety net and become a second archive of exactly the material
        // somebody asked to have destroyed.
        done.expired += ops.expire_snapshots(now)?;

        ops.due_processed(now)?;
        Ok(done)
    }

    /// Append one event of any kind, committing it and updating the projection.
    ///
    /// The body is built by a closure because every body opens with a fresh
    /// nonce, and the nonce has to come from here — it is what makes the body's
    /// hash a hiding commitment, so a redacted event cannot be confirmed by
    /// guessing what it said.
    pub fn append<F>(
        &mut self,
        kind: Kind,
        tgt: Option<capture_core::EventId>,
        prev: Option<capture_core::EventId>,
        body: F,
    ) -> Result<capture_core::EventId>
    where
        F: FnOnce([u8; 16]) -> Vec<u8>,
    {
        let nonce = entropy::nonce()?;
        let e = self.author(kind, tgt, prev, body(nonce.0))?;
        let id = e.header.id();
        self.commit(&[e])?;
        Ok(id)
    }

    /// Order the destruction of a note, and record who it has to reach.
    ///
    /// The two halves are one call because they must not come apart. Appending
    /// the event without recording the pending set leaves a redaction that
    /// every surface reports as complete the moment it is written — for a
    /// device that has not received it and may never — and that is the one
    /// claim a redaction must never make falsely.
    ///
    /// This exists because there were two places authoring `redact` and only
    /// one of them recorded anything, which is exactly the second-path problem
    /// the ingest gate exists to avoid.
    pub fn redact(&mut self, tgt: capture_core::EventId) -> Result<capture_core::EventId> {
        let id = self.append(Kind::REDACT, Some(tgt), None, |nonce| {
            let body = capture_core::body::RedactBody {
                nonce,
                scope: capture_core::body::RedactScope::WholeNote,
                targets: alloc_vec(tgt),
            };
            let mut b = Vec::new();
            minicbor::encode(&body, &mut b).expect("encoding into Vec is infallible");
            b
        })?;

        // Fixed now, and never recomputed from the roster. The realistic
        // sequence is that the phone is stolen, you retire it, and *then* you
        // redact — and a set read from the roster at report time would drop
        // the phone and announce success for the one device that never
        // received anything (§5.4).
        //
        // Membership from the log, not the projection: the projection's copy
        // is only written when a gate opens, and a local capture never opens
        // one, so a device that had not yet ingested from a peer would record
        // the redaction as owed to nobody.
        let me = self.identity.public().to_bytes();
        let members: Vec<[u8; 32]> = capture_core::roster::build(&self.log.all()?)
            .keys
            .into_iter()
            .filter(|k| k != &me)
            .collect();

        if !members.is_empty() {
            Ops::open(&self.paths.ops)?.redact_awaits(&id, &members, now_ms())?;
        }
        Ok(id)
    }

    /// Retire a key: stop accepting anything it authors beyond where it is now.
    ///
    /// Retires the **key**, not an origin. Retiring an origin would let a
    /// stolen device mint a fresh incarnation and walk straight back in, which
    /// would make the whole remedy evaporate on one restart (§5.3).
    ///
    /// Names a head per origin rather than a moment. The retired device
    /// chooses its own `hlc` and can backdate, and there is no causal ordering
    /// between a retirement and the events of the device being retired — so a
    /// timestamp rule is unenforceable. The per-origin chain makes "beyond
    /// this event" provable whatever the clock claims.
    pub fn retire(
        &mut self,
        key: &[u8; 32],
        supersedes: Option<capture_core::EventId>,
    ) -> Result<capture_core::EventId> {
        let roster = capture_core::roster::build(&self.log.all()?);

        // Where each of that key's origins stops, as this device sees it. Read
        // from the log rather than asked for: the point of a head is that it is
        // what *was* legitimately authored, and a number supplied by hand would
        // be a guess about somebody else's history.
        let heads = self.log.heads()?;
        let mut named = Vec::new();
        for (dev, owner) in &roster.origins {
            if owner != key {
                continue;
            }
            let hex = crate::projection::dev_hex(dev);
            let Some(&seq) = heads.get(&hex) else {
                continue;
            };
            let Some(last) = self
                .log
                .range(&hex, seq, seq)?
                .first()
                .map(|e| e.header.id())
            else {
                continue;
            };
            named.push(capture_core::body::RetiredHead {
                dev: *dev,
                seq,
                last,
            });
        }

        let key = *key;
        self.append(Kind::DEVICE_RETIRE, None, None, move |nonce| {
            let body = capture_core::body::RetireBody {
                nonce,
                pubkey: key,
                heads: named,
                supersedes,
            };
            let mut b = Vec::new();
            minicbor::encode(&body, &mut b).expect("encoding into Vec is infallible");
            b
        })
    }

    /// Re-admit a key, co-signed by a second one.
    ///
    /// The only event whose validity does not descend from current roster
    /// membership, and it exists because any member may retire any member: one
    /// compromised or malfunctioning device can retire every other, after which
    /// no `device_add` can help because there are no members left to sign one.
    ///
    /// The co-signature covers the subject rather than the event id, because
    /// the id covers the body hash and the body is where the co-signature
    /// lives. Forging one needs two compromised devices *and* being in the room
    /// with them.
    pub fn readmit_with(
        &mut self,
        subject: &Admitted,
        cosigner: &[u8; 32],
        cosign: &[u8; 64],
        nonce: [u8; 16],
    ) -> Result<capture_core::EventId> {
        if cosigner == &subject.pubkey {
            return Err(crate::Error::Usage(
                "a re-admission needs two different keys — the subject cannot vouch for itself"
                    .into(),
            ));
        }

        // The nonce is supplied rather than minted here. Every other body gets
        // a fresh one from this layer, and this is the exception: the
        // co-signature covers the nonce, and the two devices signing have no
        // channel to agree one over beyond the person carrying it between them.
        let (pubkey, incarnation) = (subject.pubkey, subject.incarnation);
        let (cosigner, cosign) = (*cosigner, *cosign);
        self.append(Kind::DEVICE_READMIT, None, None, move |_| {
            let body = capture_core::body::ReadmitBody {
                nonce,
                pubkey,
                incarnation,
                cosigner,
                cosign,
            };
            let mut b = Vec::new();
            minicbor::encode(&body, &mut b).expect("encoding into Vec is infallible");
            b
        })
    }

    /// Admit another device: this one vouching for its key.
    ///
    /// The event a pairing produces. Every other device learns the newcomer by
    /// folding this rather than by pairing with it, which is what makes
    /// pairing one physical gesture instead of one per pair of devices (§5.3).
    ///
    /// The subject's origin is not named — it is derived from the key and
    /// incarnation — so an admission cannot claim an origin the subject does
    /// not own, including one another device is already using.
    pub fn admit(&mut self, subject: &Admitted) -> Result<capture_core::EventId> {
        let (pubkey, incarnation, noise) = (subject.pubkey, subject.incarnation, subject.noise);
        self.append(Kind::DEVICE_ADD, None, None, move |nonce| {
            let body = capture_core::body::OriginClaimBody {
                nonce,
                pubkey,
                incarnation,
                // Without it the newcomer is a member nobody can dial.
                noise: Some(noise),
            };
            let mut b = Vec::new();
            minicbor::encode(&body, &mut b).expect("encoding into Vec is infallible");
            b
        })
    }

    /// Author several events of one kind and make them durable together.
    ///
    /// One commit rather than one per event, which matters because a commit is
    /// not cheap: it appends to the log, re-folds what the arrivals touched,
    /// advances the identity and marks the device active — several durable
    /// writes across three databases, about seventy milliseconds of them.
    ///
    /// The review session is what asked for this. Offering five notes writes
    /// five `shown` events, and doing that one at a time cost a third of a
    /// second before the pane could draw — a delay on opening a tab, which is
    /// exactly where somebody notices one.
    pub fn append_all<F>(
        &mut self,
        kind: Kind,
        targets: &[capture_core::EventId],
        body: F,
    ) -> Result<Vec<capture_core::EventId>>
    where
        F: Fn([u8; 16], capture_core::EventId) -> Vec<u8>,
    {
        let mut events = Vec::with_capacity(targets.len());
        for tgt in targets {
            let nonce = entropy::nonce()?;
            let e = self.author(kind, Some(*tgt), None, body(nonce.0, *tgt))?;

            // Appended as it is authored. The next event's sequence and chain
            // are read back from the log, so they are only right once this one
            // is in it.
            self.log.append(&e)?;
            events.push(e);
        }
        if events.is_empty() {
            return Ok(Vec::new());
        }

        // The expensive half, once for the batch.
        self.settle(&events)?;
        Ok(events.iter().map(|e| e.header.id()).collect())
    }

    /// Author an event under this device's origin.
    fn author(
        &mut self,
        kind: Kind,
        tgt: Option<capture_core::EventId>,
        prev: Option<capture_core::EventId>,
        body: Vec<u8>,
    ) -> Result<Envelope> {
        let dev = self.identity.dev();
        let seq = self.next_seq()?;
        let hlc = self.clock.issue(now_ms(), dev);

        let header = Header {
            v: ENVELOPE_VERSION,
            dev,
            seq,
            hlc,
            kind,
            tgt,
            prev,
            occ: None,
            chain: self.chain_tip()?,
            bhash: BodyHash::of(&body),
        };
        let sig = self.identity.signing().sign(&header.id().0).to_bytes();
        Ok(Envelope {
            header,
            body: Some(body),
            sig,
        })
    }

    /// Append events, update the projection, then record progress.
    ///
    /// The order is the durability guarantee. The log commits first because it
    /// is the only thing that cannot be reconstructed; the projection is
    /// rebuilt from it for free, and the watermark is allowed to lag.
    fn commit(&mut self, events: &[Envelope]) -> Result<()> {
        for e in events {
            self.log.append(e)?;
        }
        self.settle(events)
    }

    /// Everything a commit does after the events are in the log.
    ///
    /// Split out because it is the expensive half and it does not have to be
    /// paid per event: the re-fold, the identity advance and the activity mark
    /// are each once-per-batch work. What cannot be shared is appending, since
    /// an event's sequence and chain are read back from the log — authoring
    /// several before appending any gives them all the same number, and the
    /// integrity check says so.
    fn settle(&mut self, events: &[Envelope]) -> Result<()> {
        incremental::apply(&mut self.state, &self.log, events, now_ms() as i64)?;

        if let Some(last) = events.last() {
            let (wall, counter) = self.clock.persist();
            self.identity
                .advance(&self.paths, last.header.seq, wall, counter)?;
        }

        // Nothing records "the device was used" here any more.
        //
        // It used to: a timestamp written to `ops.db` on every commit, which
        // cost a disk sync — thirteen milliseconds of every write — for a copy
        // of something the log had already stored one column away. Every event
        // carries the device that wrote it and the moment it was written, so
        // `Log::last_wall` derives the same answer when the question is asked,
        // which is rarely, instead of paying for it on every write.
        Ok(())
    }

    fn next_seq(&self) -> Result<u64> {
        next_seq(&self.log, &self.identity.dev())
    }

    fn chain_tip(&self) -> Result<Option<capture_core::EventId>> {
        chain_tip(&self.log, &self.identity.dev())
    }
}

/// The next free position in an origin's sequence.
///
/// From the log's heads, never from the watermark: the watermark is written
/// after the event commits and is allowed to lag, so reading it would hand back
/// a number that is already taken.
fn next_seq(log: &Log, dev: &capture_core::Dev) -> Result<u64> {
    Ok(log.heads()?.get(&dev_hex(dev)).copied().unwrap_or(0) + 1)
}

fn chain_tip(log: &Log, dev: &capture_core::Dev) -> Result<Option<capture_core::EventId>> {
    log.chain_tip(dev)
}

/// What a capture turned into.
pub struct Captured {
    /// The note's permanent name.
    pub slug: String,
}

/// Build a capture body, without needing a store.
///
/// Deliberately free of any store handle. Everything a capture body holds — the
/// nonce, the zone, the ambient facts — comes from this machine, so a capture
/// can be fully formed and written to the spool even when the store cannot be
/// opened at all. That case is not exotic: a disk that has filled up is exactly
/// a disk whose databases will not open.
pub fn capture_body(text: &str) -> Result<Vec<u8>> {
    capture_body_in(text, None)
}

/// The same, carrying where the text came from.
///
/// For a share-style capture: something was shared out of another application
/// and the URL it came from is worth keeping, because a quotation without its
/// source is a thing you cannot go back to. §2.3's envelope has carried a
/// `source_url` field since M1 with nothing able to fill it — the desktop has
/// no share gesture, and the phone is the first surface that does.
pub fn capture_body_from(text: &str, source_url: Option<&str>) -> Result<Vec<u8>> {
    let nonce = entropy::nonce()?;
    let mut ambient = ambient::collect(&ambient::Host);
    ambient.source_url = source_url.map(String::from);

    Ok(encode(&CaptureBody {
        nonce: nonce.0,
        text: text.to_string(),
        tz: Some(local_zone()),
        ambient,
        blob: None,
    }))
}

/// The same, carrying a zone chosen by the caller.
///
/// Used where a capture continues an existing one rather than being made now —
/// splitting a recurring series, for instance. Inheriting the original's zone
/// keeps the two halves on the same clock; taking this machine's would move the
/// continuation to wherever the split happened to be performed.
pub fn capture_body_in(text: &str, tz: Option<&str>) -> Result<Vec<u8>> {
    let nonce = entropy::nonce()?;
    Ok(encode(&CaptureBody {
        nonce: nonce.0,
        text: text.to_string(),
        // Recorded now because it cannot be recovered later, and day grouping
        // depends on it: a note captured at 23:40 in one zone must not move to
        // a different day when read in another.
        tz: Some(tz.map(String::from).unwrap_or_else(local_zone)),
        ambient: ambient::collect(&ambient::Host),
        blob: None,
    }))
}

/// Milliseconds since the epoch.
///
/// The single wall-clock read in the writing path. Everything below takes the
/// value as an argument, which is what makes the fold reproducible: it cannot
/// consult a clock even by accident, because it cannot name one.
fn now_ms() -> u64 {
    jiff::Timestamp::now().as_millisecond().max(0) as u64
}

/// The IANA zone this machine is set to, or `UTC` if it cannot be determined.
///
/// Falling back rather than failing: a capture must not be refused because the
/// system has no zone configured. A wrong zone misfiles a note by a day at
/// worst; a refused capture loses it.
fn local_zone() -> String {
    jiff::tz::TimeZone::system()
        .iana_name()
        .unwrap_or("UTC")
        .to_string()
}

pub(crate) fn encode<T: minicbor::Encode<()>>(v: &T) -> Vec<u8> {
    let mut b = Vec::new();
    minicbor::encode(v, &mut b).expect("encoding into Vec is infallible");
    b
}

fn dev_hex(d: &capture_core::Dev) -> String {
    d.0.iter().map(|b| format!("{b:02x}")).collect()
}

/// What became of a capture.
pub enum Committed {
    /// In the log, under this name.
    Stored(String),
    /// In the spool, because the store would not take it. Safe, not yet
    /// findable, and committed on the next successful write.
    Spooled,
}

/// Store a capture, falling back to the spool if anything at all goes wrong.
///
/// # Why the body is built before the store is opened
///
/// Opening is one of the things that can fail — a full disk is exactly a disk
/// whose databases will not open — and a capture that only exists after a
/// successful open has nothing left to rescue when the open is what failed. So
/// everything the body needs comes from this machine: the nonce, the zone, the
/// ambient facts.
///
/// # Why a spooled capture is not an error
///
/// The interface accepted it and it is durable. Reporting failure would send
/// someone off to retype something that is already safe, which is a worse
/// outcome than the delay in it becoming searchable.
pub fn capture(paths: &StorePaths, text: &str) -> Result<Committed> {
    commit_body(paths, &capture_body(text)?)
}

/// Store a body that has already been built.
///
/// The spool fallback lives here rather than in each caller, because there is
/// now more than one way to build a capture body — a typed one and a shared
/// one — and two copies of "and if that fails, spool it" is one copy that
/// eventually does not.
pub fn commit_body(paths: &StorePaths, body: &[u8]) -> Result<Committed> {
    match Session::open_for_capture(paths.clone()).and_then(|mut s| s.capture(body)) {
        Ok(c) => Ok(Committed::Stored(c.slug)),
        Err(store_failed) => {
            spool::push(&paths.spool, body).map_err(|spool_failed| {
                // Both places gone. The only case where a capture is genuinely
                // lost, so it says so rather than reporting the second failure
                // and quietly hiding the first.
                Error::Usage(format!(
                    "could not store this capture and could not spool it either.\n\
                     \n  store:  {store_failed}\n  spool:  {spool_failed}"
                ))
            })?;
            tracing::warn!(%store_failed, "capture spooled");
            Ok(Committed::Spooled)
        }
    }
}
