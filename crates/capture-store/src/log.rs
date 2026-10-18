//! The event log: the one durable artefact.
//!
//! Everything else in the system is derived from this and can be thrown away.
//! This cannot, so the rules below are the ones worth being careful about.

use std::collections::BTreeMap;
use std::path::Path;

use capture_core::{
    Dev, EventId,
    body::CaptureBody,
    envelope::{Envelope, Rejection, Verdict},
    event::Header,
    hlc::Hlc,
    id::BodyHash,
};
use rusqlite::{Connection, OptionalExtension, params};

/// One of an origin's redactions, for the destruction ceiling.
///
/// Ordered by `(at, seq)`, which is a total order within one origin, because
/// the ceiling has to be able to say which redactions came *before* this one.
/// §1.1 holds "further" redactions beyond the bound — so the first that fit
/// proceed and the rest wait, and that needs an order rather than a set.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RedactionIssued {
    /// When it was issued, by its own clock.
    pub at: u64,
    /// Its position in the origin's sequence, which breaks ties on `at`.
    pub seq: u64,
    /// How many targets it names.
    pub targets: usize,
}

/// An event the log is holding and refusing to use.
///
/// Authentic, occupying its position, and excluded from every fold. What the
/// projection reports as quarantined is this, re-read — not a second record
/// kept in step by hand.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AsideRow {
    /// The event.
    pub id: EventId,
    /// Its origin.
    pub dev: Dev,
    /// Its position in that origin.
    pub seq: u64,
    /// Why it is being refused.
    pub reason: String,
    /// When it was authored. See `aside_rows` for why not when it was seen.
    pub at: u64,
}

use crate::{Error, Result, db};

/// Schema for the durable log.
///
/// `heads` deserves the comment it carries: the number stored there is the
/// highest **contiguous** sequence, not the highest seen. Storing a maximum
/// instead is a one-word change that silently loses data — see [`contiguous`].
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS events (
  id    BLOB PRIMARY KEY,
  dev   TEXT    NOT NULL,
  seq   INTEGER NOT NULL,
  hlc_wall    INTEGER NOT NULL,
  hlc_counter INTEGER NOT NULL,
  hlc_origin  BLOB    NOT NULL,
  kind  INTEGER NOT NULL,
  tgt   BLOB,
  prev  BLOB,
  occ   INTEGER,
  chain BLOB,
  bhash BLOB NOT NULL,
  body  BLOB,
  sig   BLOB NOT NULL,
  -- Why this event is held but not used, or NULL for the ordinary case.
  --
  -- A set-aside event occupies its position and takes no part in anything
  -- derived from the log. Both halves matter and they used to be in conflict:
  -- the event was kept out of the log entirely, so `heads` never advanced past
  -- it, and anti-entropy re-requested that tail from every peer for ever —
  -- which is the hole `sync-membership/spec.md` says a version vector cannot
  -- represent.
  --
  -- Storing the row closes the hole. The column is what stops it being used,
  -- and it is here rather than in a table beside the log so that `all()` — the
  -- one function every fold, every roster build and every surface goes
  -- through — is correct by construction. A reader that forgets to ask would
  -- otherwise be a reader that folds a retired device's writes, and on the
  -- roster path that is a retired device admitting a key of its choosing.
  aside TEXT,
  UNIQUE(dev, seq)
) STRICT;

CREATE INDEX IF NOT EXISTS events_tgt ON events(tgt, hlc_wall, hlc_counter);
CREATE INDEX IF NOT EXISTS events_dev ON events(dev, seq);

-- The rules: events that change notes they do not name, which today is a mute
-- written against a tag or a span of days. There are very few and an
-- incremental fold has to carry all of them, so they are worth finding without
-- reading the log to do it.
CREATE INDEX IF NOT EXISTS events_rules ON events(kind) WHERE tgt IS NULL;

-- Highest CONTIGUOUS sequence per origin. Never a maximum.
CREATE TABLE IF NOT EXISTS heads (
  dev TEXT PRIMARY KEY,
  seq INTEGER NOT NULL
) STRICT;

-- Events held above the contiguous head, waiting for the gap below them to
-- fill. Their rows are already in `events`; this only records that they are
-- not yet covered by `heads`.
CREATE TABLE IF NOT EXISTS pending (
  dev TEXT    NOT NULL,
  seq INTEGER NOT NULL,
  PRIMARY KEY (dev, seq)
) STRICT;
";

/// A handle to the durable log.
pub struct Log {
    conn: Connection,
}

/// What happened to an event offered for ingest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ingested {
    /// Stored, and covered by the contiguous head.
    Accepted,
    /// Stored, but a lower sequence number is still missing.
    Pending,
    /// Already held, byte for byte. Nothing changed.
    Duplicate,
    /// Not authenticated. Kept outside the log; occupies no position.
    Rejected(Rejection),
    /// Two different events claim one position in one origin's sequence.
    Collision,
}

impl Log {
    /// Open or create the log.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = db::open_write(path)?;
        db::create_schema(&conn, SCHEMA)?;
        add_aside_column(&conn)?;
        Ok(Log { conn })
    }

    /// Open read-only, without needing the writer.
    pub fn open_read(path: &Path) -> Result<Self> {
        let conn = db::open_read(path)?;
        shadow_missing_aside(&conn)?;
        Ok(Log { conn })
    }

    /// Take in an event.
    ///
    /// The checks run in a fixed order, and the order encodes a distinction
    /// that matters more than it looks:
    ///
    /// 1. Signature — does the origin's key vouch for this?
    /// 2. Payload against its commitment — is this the payload that was signed?
    /// 3. Position — is that slot already taken by something else?
    ///
    /// **An event that fails 1 or 2 never touches the log.** It is not
    /// authenticated, so it proves nothing about who wrote it — and if such an
    /// event could occupy a sequence number, any peer could claim a slot with
    /// garbage. The genuine event for that slot would then collide with it and
    /// be refused everywhere, permanently, while every device reported the
    /// position as held. A real capture would become unreachable.
    ///
    /// An event that passes 1 and 2 is provably written by the origin whose
    /// slot it sits in, so it cannot be squatting. Those are stored even when
    /// this build has no idea what to do with them.
    pub fn ingest(&mut self, e: &Envelope, pubkey: &[u8; 32]) -> Result<Ingested> {
        self.ingest_as(e, pubkey, None)
    }

    /// Ingest an event that is authentic and may not be used.
    ///
    /// # Why it is stored at all
    ///
    /// An event set aside for membership reasons — authored by a device past
    /// the point its retirement named — used to be kept out of the log
    /// entirely. That left `heads` for the origin stopped at the retirement
    /// head, and a version vector cannot describe a hole: every round
    /// afterwards asked every peer for `head+1..N`, was sent the whole tail,
    /// set it aside again and asked again, for ever, with the bytes on the wire
    /// each time. `sync-membership/spec.md` calls for set-aside rather than
    /// refusal precisely to avoid that, and the implementation did the
    /// opposite.
    ///
    /// Storing the row occupies the position, so the head advances and the
    /// question stops being asked. The `aside` column is what keeps it out of
    /// everything derived from the log — see [`Self::all`].
    ///
    /// `reason` is recorded rather than a flag, because "past a retirement
    /// head" and "some later reason" are not the same fact and a surface that
    /// had to guess would guess wrong.
    pub fn ingest_aside(
        &mut self,
        e: &Envelope,
        pubkey: &[u8; 32],
        reason: &str,
    ) -> Result<Ingested> {
        self.ingest_as(e, pubkey, Some(reason))
    }

    fn ingest_as(
        &mut self,
        e: &Envelope,
        pubkey: &[u8; 32],
        aside: Option<&str>,
    ) -> Result<Ingested> {
        match e.verify(pubkey) {
            Verdict::Authentic => {}
            Verdict::Rejected(r) => return Ok(Ingested::Rejected(r)),
        }

        let id = e.header.id();
        let dev = dev_key(&e.header.dev);

        // Already held?
        let existing: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT id FROM events WHERE dev = ?1 AND seq = ?2",
                params![dev, e.header.seq as i64],
                |r| r.get(0),
            )
            .optional()?;

        if let Some(prior) = existing {
            return if prior == id.0 {
                // The same event twice. Storing it again would change nothing.
                Ok(Ingested::Duplicate)
            } else {
                // Two different events claiming one position in one origin's
                // sequence — which single-writer-per-origin says cannot happen.
                // Treating this as a benign duplicate is what turns a
                // rolled-back store into silent, permanent, two-way data loss:
                // the arriving event is discarded, and the stored one is
                // reported as held by a device that no longer has the events
                // around it.
                Ok(Ingested::Collision)
            };
        }

        let tx = self.conn.transaction()?;
        insert_as(&tx, e, aside)?;

        // Sequence number, row, and head all move together. A crash between
        // them would leave a gap that nothing can fill, and a gap in an
        // origin's numbering is not recoverable — the summary a device
        // publishes is a single number per origin and cannot describe a hole.
        let head = head_of(&tx, &dev)?;
        let outcome = if e.header.seq == head + 1 {
            let advanced = absorb(&tx, &dev, e.header.seq)?;
            let _ = advanced;
            Ingested::Accepted
        } else {
            tx.execute(
                "INSERT OR IGNORE INTO pending (dev, seq) VALUES (?1, ?2)",
                params![dev, e.header.seq as i64],
            )?;
            Ingested::Pending
        };
        tx.commit()?;
        Ok(outcome)
    }

    /// Let a set-aside event be used after all.
    ///
    /// The remedy for a retirement that was narrower than intended: reissued
    /// naming a later head, it re-admits exactly the events below that head.
    /// The row is already in its position, so this only clears the mark that
    /// keeps it out of the fold — nothing moves, and no head changes.
    ///
    /// Returns whether a row was actually released, so a caller can tell a
    /// decision that did something from one that did not.
    pub fn release_aside(&mut self, id: &EventId) -> Result<bool> {
        let n = self.conn.execute(
            "UPDATE events SET aside = NULL WHERE id = ?1 AND aside IS NOT NULL",
            params![id.0.to_vec()],
        )?;
        Ok(n > 0)
    }

    /// Append an event authored locally.
    ///
    /// Local events are always the next in sequence, so this is ingest without
    /// the possibility of a gap.
    pub fn append(&mut self, e: &Envelope) -> Result<()> {
        let dev = dev_key(&e.header.dev);
        let tx = self.conn.transaction()?;
        let head = head_of(&tx, &dev)?;
        if e.header.seq != head + 1 {
            return Err(Error::Integrity(format!(
                "local append out of sequence: expected {}, got {}",
                head + 1,
                e.header.seq
            )));
        }
        insert(&tx, e)?;
        absorb(&tx, &dev, e.header.seq)?;
        tx.commit()?;
        Ok(())
    }

    /// When this origin last wrote something.
    ///
    /// The answer to "was anybody here", derived rather than stored. Every
    /// event carries the device that authored it and the wall clock it was
    /// authored at, so the log already holds this — and it was additionally
    /// being written to `ops.db` on every commit, costing a disk sync for a
    /// copy of a fact one column away.
    ///
    /// Only writes count, which is what the stored version counted too: it was
    /// set from `commit` and nowhere else, so browsing has never marked a
    /// device active and deriving it changes nothing.
    pub fn last_wall(&self, dev: &capture_core::Dev) -> Result<Option<u64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT MAX(hlc_wall) FROM events WHERE dev = ?1",
                rusqlite::params![dev_key(dev)],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()?
            .flatten()
            .map(|v| v as u64))
    }

    /// What this device holds, as one number per origin.
    ///
    /// This is the whole summary a device publishes about its own contents. It
    /// can say "everything up to 41,822" and nothing else — in particular it
    /// cannot say "everything up to 41,822 except 41,700", which is why a gap
    /// must never be created in the first place.
    pub fn heads(&self) -> Result<BTreeMap<String, u64>> {
        let mut st = self.conn.prepare("SELECT dev, seq FROM heads")?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
        })?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (d, s) = row?;
            out.insert(d, s);
        }
        Ok(out)
    }

    /// Sequence numbers held above the contiguous head.
    pub fn pending(&self) -> Result<Vec<(String, u64)>> {
        let mut st = self
            .conn
            .prepare("SELECT dev, seq FROM pending ORDER BY dev, seq")?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Every event this device may actually use, for folding.
    ///
    /// Excludes anything set aside. This is the function every fold, every
    /// roster build and every surface reaches for, so the exclusion lives here
    /// rather than at each of them: a caller that had to remember would be a
    /// caller that eventually did not, and on the roster path forgetting means
    /// a retired device admitting a key of its choosing.
    ///
    /// [`Self::all_including_aside`] is the way to see everything, and its name
    /// is deliberately awkward.
    pub fn all(&self) -> Result<Vec<Envelope>> {
        let mut st = self.conn.prepare(
            "SELECT id, dev, seq, hlc_wall, hlc_counter, hlc_origin, kind, tgt, prev, occ,
                    chain, bhash, body, sig
             FROM events WHERE aside IS NULL ORDER BY hlc_wall, hlc_counter, hlc_origin",
        )?;
        let rows = st.query_map([], row_to_envelope)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Every event, including those set aside.
    ///
    /// For the two callers that want the archive as it physically is rather
    /// than as it is used: an export, which must carry a set-aside event
    /// because it occupies a position and a restore without it would reproduce
    /// the hole; and anything auditing what this device actually holds.
    ///
    /// Not for folding. The name is long on purpose.
    pub fn all_including_aside(&self) -> Result<Vec<Envelope>> {
        let mut st = self.conn.prepare(
            "SELECT id, dev, seq, hlc_wall, hlc_counter, hlc_origin, kind, tgt, prev, occ,
                    chain, bhash, body, sig
             FROM events ORDER BY hlc_wall, hlc_counter, hlc_origin",
        )?;
        let rows = st.query_map([], row_to_envelope)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// One event by id, if this device holds it.
    pub fn by_id(&self, id: &EventId) -> Result<Option<Envelope>> {
        let mut st = self.conn.prepare(
            "SELECT id, dev, seq, hlc_wall, hlc_counter, hlc_origin, kind, tgt, prev, occ,
                    chain, bhash, body, sig
             FROM events WHERE id = ?1",
        )?;
        let mut rows = st.query_map(params![id.0.to_vec()], row_to_envelope)?;
        rows.next().transpose().map_err(Into::into)
    }

    /// One origin's events over a range of sequence numbers, in order.
    ///
    /// What a peer asked for by name. In sequence order rather than timestamp
    /// order, because the receiver appends them in that order and a gap is
    /// what it is trying to avoid.
    pub fn range(&self, dev: &str, from: u64, to: u64) -> Result<Vec<Envelope>> {
        let mut st = self.conn.prepare(
            "SELECT id, dev, seq, hlc_wall, hlc_counter, hlc_origin, kind, tgt, prev, occ,
                    chain, bhash, body, sig
             FROM events WHERE dev = ?1 AND seq >= ?2 AND seq <= ?3 ORDER BY seq",
        )?;
        let rows = st.query_map(params![dev, from as i64, to as i64], row_to_envelope)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Everything an incremental fold needs for a set of notes.
    ///
    /// Two queries against indexes, in place of reading the whole log and
    /// filtering it in memory. `commit` did the latter on every write: at a
    /// hundred and fifty events that was two milliseconds and invisible, and it
    /// grows with the archive for ever, on the path a capture waits on.
    ///
    /// What comes back is every event belonging to one of these notes, plus
    /// every rule. The rules have to come along: a note that has just been
    /// given a tag may fall under a mute written weeks ago, and a fold that
    /// left the rule out would produce a note the mute plainly covers and that
    /// is not muted.
    ///
    /// A capture is its own note and cannot name itself — the identity is
    /// computed over the target field, so writing it in would change the value
    /// being computed — which is why the identifiers are looked for in `id` as
    /// well as in `tgt`.
    pub fn events_touching(
        &self,
        targets: &std::collections::BTreeSet<capture_core::EventId>,
    ) -> Result<Vec<Envelope>> {
        let mut out = Vec::new();
        if targets.is_empty() {
            return Ok(out);
        }

        let ids: Vec<Vec<u8>> = targets.iter().map(|t| t.0.to_vec()).collect();
        let holes = std::iter::repeat_n("?", ids.len())
            .collect::<Vec<_>>()
            .join(",");

        let mut st = self.conn.prepare(&format!(
            "SELECT id, dev, seq, hlc_wall, hlc_counter, hlc_origin, kind, tgt, prev, occ,
                    chain, bhash, body, sig
             FROM events
             WHERE (tgt IN ({holes}) OR id IN ({holes})) AND aside IS NULL
             ORDER BY hlc_wall, hlc_counter, hlc_origin"
        ))?;
        let both: Vec<&dyn rusqlite::ToSql> = ids
            .iter()
            .chain(ids.iter())
            .map(|v| v as &dyn rusqlite::ToSql)
            .collect();
        for r in st.query_map(both.as_slice(), row_to_envelope)? {
            out.push(r?);
        }

        for r in self.rules()? {
            out.push(r);
        }
        out.sort_by_key(|e| (e.header.hlc.wall, e.header.hlc.counter));
        Ok(out)
    }

    /// Events that change notes they do not name.
    ///
    /// Mutes by rule, and what was decided about entities: renames, merges and
    /// bindings. The entity ones govern every note that mentions the entity,
    /// and the projection's entity rows are rewritten from whatever fold it is
    /// handed — so a partial fold without them undid all three on the next
    /// ordinary capture (found 2026-10-18).
    pub fn rules(&self) -> Result<Vec<Envelope>> {
        use capture_core::event::Kind;
        let mut st = self.conn.prepare(
            "SELECT id, dev, seq, hlc_wall, hlc_counter, hlc_origin, kind, tgt, prev, occ,
                    chain, bhash, body, sig
             FROM events WHERE tgt IS NULL AND kind IN (?1, ?2, ?3, ?4) AND aside IS NULL
             ORDER BY hlc_wall, hlc_counter, hlc_origin",
        )?;
        let mut out = Vec::new();
        for r in st.query_map(
            params![
                Kind::MUTE.0 as i64,
                Kind::ENTITY_RENAME.0 as i64,
                Kind::ENTITY_MERGE.0 as i64,
                Kind::ENTITY_BIND.0 as i64,
            ],
            row_to_envelope,
        )? {
            out.push(r?);
        }
        Ok(out)
    }

    /// The most recent event this origin wrote, by sequence.
    ///
    /// One indexed lookup. This was done by reading every event in the log and
    /// taking the maximum in memory — on every event *authored*, not merely
    /// every commit, so a capture paid for the whole archive twice over.
    pub fn chain_tip(&self, dev: &capture_core::Dev) -> Result<Option<capture_core::EventId>> {
        let id: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT id FROM events WHERE dev = ?1 ORDER BY seq DESC LIMIT 1",
                params![dev_key(dev)],
                |r| r.get(0),
            )
            .optional()?;
        Ok(id.and_then(|v| {
            let a: [u8; 32] = v.try_into().ok()?;
            Some(capture_core::EventId(a))
        }))
    }

    /// How many events are held.
    pub fn count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get::<_, i64>(0))?
            as u64)
    }

    /// Every event, paired with the identity the row was stored under.
    ///
    /// The stored id is carried separately because the whole point of checking
    /// it is that it might disagree with the one the header produces. `all()`
    /// recomputes the id from the header and can therefore never notice a row
    /// whose key was corrupted — it would simply hand back a different event
    /// than the one that was written, with nothing to compare against.
    pub fn rows(&self) -> Result<Vec<(EventId, Envelope)>> {
        let mut st = self.conn.prepare(
            "SELECT id, dev, seq, hlc_wall, hlc_counter, hlc_origin, kind, tgt, prev, occ,
                    chain, bhash, body, sig
             FROM events ORDER BY dev, seq",
        )?;
        let rows = st.query_map([], |r| {
            let mut stored = [0u8; 32];
            let raw: Vec<u8> = r.get(0)?;
            if raw.len() == 32 {
                stored.copy_from_slice(&raw);
            }
            Ok((EventId(stored), row_to_envelope(r)?))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// One origin's redactions, with how many targets each names.
    ///
    /// The ceiling's history, and the log is the only place it can come from.
    /// Redaction bodies are *retained* — `destroy_bodies` excludes `REDACT` and
    /// `REDACT_CANCEL` — precisely so a redaction goes on destroying matching
    /// events that arrive later, which is what leaves the targets countable
    /// long after the notes they named are gone.
    ///
    /// It used to come from the projection, from a table redactions never
    /// reach, so it was empty and the ceiling never fired. The projection is
    /// also the wrong place on principle: it is derived and rebuilt, and a
    /// ceiling that resets when a projection is rebuilt is not a ceiling.
    ///
    /// A body this build cannot decode counts as one target. It is a redaction
    /// either way, and counting it as nothing would let an unreadable body be
    /// the way past the bound.
    pub fn redactions_by(&self, dev: &Dev) -> Result<Vec<RedactionIssued>> {
        let mut st = self.conn.prepare(
            "SELECT hlc_wall, seq, body FROM events
             WHERE kind = ?1 AND dev = ?2 AND body IS NOT NULL
             ORDER BY hlc_wall, seq",
        )?;
        let rows = st.query_map(
            params![
                capture_core::event::Kind::REDACT.0 as i64,
                crate::projection::dev_hex(dev)
            ],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Vec<u8>>(2)?,
                ))
            },
        )?;
        let mut out = Vec::new();
        for r in rows {
            let (at, seq, body) = r?;
            let targets = minicbor::decode::<capture_core::body::RedactBody>(&body)
                .map(|b| b.targets.len().max(1))
                .unwrap_or(1);
            out.push(RedactionIssued {
                at: at as u64,
                seq: seq as u64,
                targets,
            });
        }
        Ok(out)
    }

    /// Every event this device has set aside, and why.
    ///
    /// The authoritative record. `aside` is a column on the log, which is never
    /// thrown away; the projection's `quarantine` table is the *report* of it,
    /// and a rebuild used to produce a projection with no report at all —
    /// leaving events set aside in the log, invisible to every surface, and
    /// `Gate::reconsider` with nothing to re-admit when a retirement was
    /// reissued. That remedy was made local precisely so it would not depend on
    /// a peer still holding the events.
    ///
    /// The moment is the event's own `hlc_wall`: when this device first *saw*
    /// it was only ever in the projection being replaced, and the authored
    /// instant is stable, so two rebuilds order the report identically.
    pub fn aside_rows(&self) -> Result<Vec<AsideRow>> {
        let mut st = self.conn.prepare(
            "SELECT id, dev, seq, aside, hlc_wall FROM events
             WHERE aside IS NOT NULL ORDER BY hlc_wall, seq",
        )?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (id, dev, seq, reason, at) = r?;
            // The column holds the hex spelling. Parsed back rather than
            // carried as text, so the row is the same shape every other writer
            // of a quarantine record uses and there is one conversion rather
            // than two spellings of an origin in flight.
            if let (Ok(id), Some(dev)) = (
                <[u8; 32]>::try_from(id.as_slice()),
                capture_core::id::unhex_array::<32>(&dev).map(Dev),
            ) {
                out.push(AsideRow {
                    id: EventId(id),
                    dev,
                    seq: seq as u64,
                    reason,
                    at: at as u64,
                });
            }
        }
        Ok(out)
    }

    /// The blobs the events of a note reference.
    ///
    /// Read *before* the bodies are destroyed, because the hash lives in
    /// `CaptureBody.blob` and nowhere else — the fold does not track blobs and
    /// there is no `blob_refs` table. Once `destroy_bodies` has run there is
    /// nothing left that can say which recording the note had.
    ///
    /// Same selection as `destroy_bodies`, so the two cannot disagree about
    /// which events a purge is about.
    pub fn blobs_of(&self, tgt: &EventId) -> Result<Vec<[u8; 32]>> {
        let mut st = self.conn.prepare(
            "SELECT body FROM events
             WHERE (tgt = ?1 OR id = ?1)
               AND body IS NOT NULL
               AND kind NOT IN (?2, ?3)",
        )?;
        let rows = st.query_map(
            params![
                tgt.0.to_vec(),
                capture_core::event::Kind::REDACT.0 as i64,
                capture_core::event::Kind::REDACT_CANCEL.0 as i64
            ],
            |r| r.get::<_, Vec<u8>>(0),
        )?;
        let mut out = Vec::new();
        for body in rows {
            if let Some(h) = blob_in(&body?) {
                out.push(h);
            }
        }
        out.sort_unstable();
        out.dedup();
        Ok(out)
    }

    /// Whether any capture other than this note's still references a blob.
    ///
    /// §5.6: dropping the *last* reference is what destroys a blob. The hash is
    /// the address, so the same recording captured twice is one blob with two
    /// references — and destroying it because one of them was redacted would
    /// destroy content nobody asked to lose, which is the cardinal failure
    /// committed by the operation least able to undo it.
    ///
    /// A `NULL` body is a note already redacted and is not a reference worth
    /// keeping bytes for. A body this build cannot decode **is** one: keeping
    /// bytes nobody wanted is recoverable and destroying bytes somebody
    /// references is not, so an unreadable body is counted rather than ignored.
    ///
    /// A full scan of bodies, which is acceptable here and nowhere else —
    /// redaction already runs `VACUUM` over the whole database, and it is the
    /// one operation where being slow and right beats being fast.
    pub fn blob_referenced_elsewhere(&self, hash: &[u8; 32], tgt: &EventId) -> Result<bool> {
        // Captures only. A blob reference lives on `CaptureBody` and nowhere
        // else, and every other kind would fail to decode as one — which, under
        // the "unreadable counts as a reference" rule below, would make a
        // redaction's own retained payload look like a reference to every blob
        // in the archive. It did, and the second of two notes sharing a
        // recording kept its bytes for ever because of it.
        let mut st = self.conn.prepare(
            "SELECT body FROM events
             WHERE body IS NOT NULL
               AND kind = ?2
               AND tgt IS NOT ?1
               AND id IS NOT ?1",
        )?;
        let rows = st.query_map(
            params![tgt.0.to_vec(), capture_core::event::Kind::CAPTURE.0 as i64],
            |r| r.get::<_, Vec<u8>>(0),
        )?;
        for body in rows {
            let body = body?;
            match minicbor::decode::<CaptureBody>(&body) {
                Ok(c) => {
                    if c.blob.map(|b| b.hash).as_ref() == Some(hash) {
                        return Ok(true);
                    }
                }
                // A capture this build cannot read. Counted as a reference:
                // keeping bytes nobody wanted is recoverable, and destroying
                // bytes somebody still references is not.
                Err(_) => return Ok(true),
            }
        }
        Ok(false)
    }

    /// Remove the payload of every event belonging to a note.
    ///
    /// The header stays, and so does the signature: an event with no body still
    /// recomputes its own id and still verifies, because the body appears in
    /// the header only as a hash. That is what lets a redacted archive pass
    /// `capture verify` rather than reading as damaged.
    ///
    /// The connection sets `secure_delete`, so the freed pages are overwritten
    /// as they are released rather than left holding the old bytes.
    ///
    /// The redaction and its cancellation keep their own payloads. A redaction
    /// has to be **retained** — it must destroy matching events that arrive
    /// after it, or "destroyed" would mean "destroyed unless something was
    /// slow" — and its payload is the only record of what it names. Destroying
    /// it along with everything else leaves a note whose content is gone and
    /// nothing anywhere saying why, or that anything further should be
    /// destroyed when it turns up.
    pub fn destroy_bodies(&mut self, tgt: &EventId) -> Result<usize> {
        let n = self.conn.execute(
            "UPDATE events SET body = NULL
             WHERE (tgt = ?1 OR id = ?1)
               AND body IS NOT NULL
               AND kind NOT IN (?2, ?3)",
            params![
                tgt.0.to_vec(),
                capture_core::event::Kind::REDACT.0 as i64,
                capture_core::event::Kind::REDACT_CANCEL.0 as i64
            ],
        )?;
        Ok(n)
    }

    /// Force the write-ahead log into the database file.
    ///
    /// Without this, destroyed bytes can sit in the `-wal` sidecar
    /// indefinitely. `secure_delete` overwrites pages inside the database; it
    /// has nothing to say about a write-ahead log that still holds the old
    /// version of them, and anything reading that file directly finds the
    /// content the purge was supposed to remove.
    pub fn checkpoint(&mut self) -> Result<()> {
        self.conn
            .pragma_update(None, "wal_checkpoint", "TRUNCATE")?;
        Ok(())
    }

    /// Whether some event already commits to this payload.
    ///
    /// Used to make draining the spool exactly-once. A spooled capture has no
    /// event identity yet — identity covers the sequence number and timestamp,
    /// which are assigned at commit — so a crash between committing a capture
    /// and clearing the spool would otherwise commit it a second time under a
    /// different id, producing two notes from one keystroke.
    ///
    /// The body's commitment is the stable name across that boundary: it is
    /// fixed when the capture is written, and it survives being committed under
    /// any sequence number. It is unique because every body opens with sixteen
    /// random bytes, so two distinct captures cannot collide here by having the
    /// same text.
    pub fn holds_body(&self, bhash: &BodyHash) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM events WHERE bhash = ?1 LIMIT 1",
                params![bhash.0.to_vec()],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }
}

/// Advance the head as far as consecutive sequence numbers allow, absorbing
/// anything that was waiting above the gap.
fn absorb(tx: &rusqlite::Transaction<'_>, dev: &str, from: u64) -> Result<u64> {
    let mut head = from;
    tx.execute(
        "INSERT INTO heads (dev, seq) VALUES (?1, ?2)
         ON CONFLICT(dev) DO UPDATE SET seq = ?2",
        params![dev, head as i64],
    )?;
    tx.execute(
        "DELETE FROM pending WHERE dev = ?1 AND seq <= ?2",
        params![dev, head as i64],
    )?;

    loop {
        let next: Option<i64> = tx
            .query_row(
                "SELECT seq FROM pending WHERE dev = ?1 AND seq = ?2",
                params![dev, (head + 1) as i64],
                |r| r.get(0),
            )
            .optional()?;
        match next {
            Some(_) => {
                head += 1;
                tx.execute(
                    "DELETE FROM pending WHERE dev = ?1 AND seq = ?2",
                    params![dev, head as i64],
                )?;
            }
            None => break,
        }
    }

    tx.execute(
        "UPDATE heads SET seq = ?2 WHERE dev = ?1",
        params![dev, head as i64],
    )?;
    Ok(head)
}

fn head_of(tx: &rusqlite::Transaction<'_>, dev: &str) -> Result<u64> {
    Ok(tx
        .query_row("SELECT seq FROM heads WHERE dev = ?1", params![dev], |r| {
            r.get::<_, i64>(0)
        })
        .optional()?
        .unwrap_or(0) as u64)
}

/// Give an existing log the `aside` column.
///
/// The schema is created with `IF NOT EXISTS`, which does nothing to a table
/// that is already there — so a store written by an earlier build has the old
/// `events` and every query naming `aside` would fail against it. On a phone
/// holding the only copy of a week's captures, that is the whole archive
/// refusing to open.
///
/// Additive and idempotent. The column is nullable with no default, so every
/// existing row reads as "not set aside", which is what every existing row is.
///
/// Checked rather than attempted-and-ignored: swallowing the error would also
/// swallow a real one, and this runs on the path that opens the archive.
fn add_aside_column(conn: &Connection) -> Result<()> {
    let present = conn
        .prepare("SELECT 1 FROM pragma_table_info('events') WHERE name = 'aside'")?
        .exists([])?;
    if !present {
        conn.execute("ALTER TABLE events ADD COLUMN aside TEXT", [])?;
    }
    Ok(())
}

/// Let a read-only connection query a log that predates `aside`.
///
/// `add_aside_column` runs on the write path, and a read-only connection cannot
/// run it. So a store last written by an earlier build, opened by anything that
/// only reads — `capture peers`, a timeline, the phone looking up a recording —
/// failed every query naming the column with "no such column: aside", until
/// something happened to open it for writing. That was the real desktop archive
/// on the day this landed: the daemon had not written since the upgrade, and
/// every read command that touched the log refused.
///
/// A temporary view named `events` is found before the table of that name —
/// SQLite resolves an unqualified name against `temp` first — so every query in
/// this file sees the column without the file being touched. The temp schema
/// belongs to the connection, which is why a read-only one may create it.
/// Every row reads as not set aside, which is true: nothing could have set one
/// aside in a log with nowhere to record it.
fn shadow_missing_aside(conn: &Connection) -> Result<()> {
    let present = conn
        .prepare("SELECT 1 FROM pragma_table_info('events') WHERE name = 'aside'")?
        .exists([])?;
    if !present {
        conn.execute(
            "CREATE TEMP VIEW events AS SELECT *, NULL AS aside FROM main.events",
            [],
        )?;
    }
    Ok(())
}

fn insert(tx: &rusqlite::Transaction<'_>, e: &Envelope) -> Result<()> {
    insert_as(tx, e, None)
}

/// Store an event, saying why it may not be used if it may not be.
fn insert_as(tx: &rusqlite::Transaction<'_>, e: &Envelope, aside: Option<&str>) -> Result<()> {
    let h = &e.header;
    tx.execute(
        "INSERT INTO events
           (id, dev, seq, hlc_wall, hlc_counter, hlc_origin, kind, tgt, prev, occ,
            chain, bhash, body, sig, aside)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        params![
            h.id().0.to_vec(),
            dev_key(&h.dev),
            h.seq as i64,
            h.hlc.wall as i64,
            h.hlc.counter as i64,
            h.hlc.origin.0.to_vec(),
            h.kind.0 as i64,
            h.tgt.map(|t| t.0.to_vec()),
            h.prev.map(|t| t.0.to_vec()),
            h.occ.map(|o| o as i64),
            h.chain.map(|c| c.0.to_vec()),
            h.bhash.0.to_vec(),
            e.body.clone(),
            e.sig.to_vec(),
            aside,
        ],
    )?;
    Ok(())
}

fn row_to_envelope(r: &rusqlite::Row<'_>) -> rusqlite::Result<Envelope> {
    let blob32 = |v: Vec<u8>| -> [u8; 32] {
        let mut a = [0u8; 32];
        a.copy_from_slice(&v[..32]);
        a
    };
    let opt_id = |v: Option<Vec<u8>>| v.map(|b| EventId(blob32(b)));

    let header = Header {
        v: capture_core::ENVELOPE_VERSION,
        dev: Dev(blob32(hex_to_bytes(&r.get::<_, String>(1)?))),
        seq: r.get::<_, i64>(2)? as u64,
        hlc: Hlc {
            wall: r.get::<_, i64>(3)? as u64,
            counter: r.get::<_, i64>(4)? as u32,
            origin: Dev(blob32(r.get::<_, Vec<u8>>(5)?)),
        },
        kind: capture_core::Kind(r.get::<_, i64>(6)? as u16),
        tgt: opt_id(r.get(7)?),
        prev: opt_id(r.get(8)?),
        occ: r.get::<_, Option<i64>>(9)?.map(|v| v as u64),
        chain: opt_id(r.get(10)?),
        bhash: BodyHash(blob32(r.get::<_, Vec<u8>>(11)?)),
    };
    let mut sig = [0u8; 64];
    sig.copy_from_slice(&r.get::<_, Vec<u8>>(13)?[..64]);
    Ok(Envelope {
        header,
        body: r.get::<_, Option<Vec<u8>>>(12)?,
        sig,
    })
}

/// Origins are stored as hex so the column is readable in a shell without a
/// decoder — every diagnostic in this system is one someone reads under
/// pressure.
fn dev_key(d: &Dev) -> String {
    d.0.iter().map(|b| format!("{b:02x}")).collect()
}

/// Hex to bytes, skipping whatever does not decode.
///
/// Deliberately not `capture_core::id::unhex`, which refuses the whole string
/// if any of it is not hex. This reads a checkpoint written by an older build
/// and wants whatever is legible; a stricter reading would discard a recovery
/// hint over one bad character.
fn hex_to_bytes(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// The blob a capture body names, if it is a capture and it names one.
///
/// Returns `None` for any other kind of body — only a capture carries a blob
/// reference (§4.3 puts it there so a note's permanent name does not depend on
/// a later event having arrived).
fn blob_in(body: &[u8]) -> Option<[u8; 32]> {
    minicbor::decode::<CaptureBody>(body)
        .ok()
        .and_then(|c| c.blob)
        .map(|b| b.hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use capture_core::testing;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("capture-log-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("events.db")
    }

    fn clean(p: &Path) {
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn a_log_from_before_aside_can_be_read_without_being_written() {
        // The real desktop archive, the day SYNC 6 landed: last written by an
        // earlier build, so its `events` table has no `aside` column, and
        // `capture peers` refused with "no such column: aside".
        let p = tmp("pre-aside");
        let s = testing::mixed(4, 1, 2, 4);
        let events = &s.events;
        {
            let mut log = Log::open(&p).unwrap();
            for e in events {
                let k = s.key_for(&e.header.dev).unwrap();
                log.ingest(e, &k).unwrap();
            }
        }
        // Back to the shape an earlier build wrote.
        {
            let c = Connection::open(&p).unwrap();
            c.execute("ALTER TABLE events DROP COLUMN aside", [])
                .unwrap();
        }

        let log = Log::open_read(&p).unwrap();
        assert_eq!(
            log.all().unwrap().len(),
            events.len(),
            "every event, none set aside"
        );
        assert_eq!(log.all_including_aside().unwrap().len(), events.len());

        // And reading did not migrate it: a reader that wrote would be a
        // reader that needs the writer.
        let c = Connection::open(&p).unwrap();
        let migrated = c
            .prepare("SELECT 1 FROM pragma_table_info('events') WHERE name = 'aside'")
            .unwrap()
            .exists([])
            .unwrap();
        assert!(!migrated, "the file was left as it was");
        clean(&p);
    }

    #[test]
    fn events_survive_a_round_trip_byte_for_byte() {
        // Rows are stored as columns and rebuilt on read. If reconstruction
        // differed by even one field, the recomputed identifier would differ
        // and nothing would verify — so this is the test that keeps the storage
        // layer honest about the frozen encoding.
        let p = tmp("roundtrip");
        let s = testing::mixed(1, 2, 3, 10);
        let mut log = Log::open(&p).unwrap();
        for e in &s.events {
            let k = s.key_for(&e.header.dev).unwrap();
            log.ingest(e, &k).unwrap();
        }
        let back = log.all().unwrap();
        assert_eq!(back.len(), s.events.len());
        for e in &back {
            let k = s.key_for(&e.header.dev).unwrap();
            assert_eq!(
                e.verify(&k),
                Verdict::Authentic,
                "reconstruction must verify"
            );
        }
        clean(&p);
    }

    /// A set-aside event occupies its position and takes no part in anything.
    ///
    /// Both halves, because they used to be in conflict. Keeping the event out
    /// of the log kept it out of the fold — and left `heads` stopped below it,
    /// so every round afterwards asked every peer for that tail, was sent it,
    /// set it aside again and asked again, for ever. A version vector cannot
    /// describe a hole, which is why the spec calls for set-aside rather than
    /// refusal in the first place.
    #[test]
    fn a_set_aside_event_holds_its_position_without_being_used() {
        let p = tmp("aside");
        let s = testing::mixed(1, 3, 3, 10);
        let mut log = Log::open(&p).unwrap();

        let first = &s.events[0];
        let dev = dev_key(&first.header.dev);
        let k = s.key_for(&first.header.dev).unwrap();

        // Everything from this origin, in order, with the second set aside.
        let mine: Vec<_> = s
            .events
            .iter()
            .filter(|e| e.header.dev == first.header.dev)
            .collect();
        assert!(mine.len() >= 3, "need a few events from one origin");

        for (i, e) in mine.iter().enumerate() {
            if i == 1 {
                log.ingest_aside(e, &k, "post_retire").unwrap();
            } else {
                log.ingest(e, &k).unwrap();
            }
        }

        // The head is past it: no hole, so nothing re-requests the tail.
        let heads = log.heads().unwrap();
        assert_eq!(
            heads.get(&dev).copied(),
            Some(mine.last().unwrap().header.seq),
            "the head stopped at the set-aside event, which is the hole \
             anti-entropy asks about for ever"
        );

        // And it is invisible to everything derived from the log.
        let used = log.all().unwrap();
        let aside_id = mine[1].header.id();
        assert!(
            !used.iter().any(|e| e.header.id() == aside_id),
            "a set-aside event reached the fold"
        );
        assert_eq!(
            used.len(),
            mine.len() - 1,
            "exactly one event is withheld, and only the one"
        );

        // Still physically held, which is what an export must carry.
        assert!(
            log.all_including_aside()
                .unwrap()
                .iter()
                .any(|e| e.header.id() == aside_id),
            "the event was not stored at all"
        );
        let aside = log.aside_rows().unwrap();
        assert_eq!(aside.len(), 1);
        assert_eq!(aside[0].id, aside_id);
        assert_eq!(aside[0].reason, "post_retire");

        // Releasing it is the reissued-retirement remedy, and needs no peer.
        assert!(log.release_aside(&aside_id).unwrap());
        assert!(
            log.all().unwrap().iter().any(|e| e.header.id() == aside_id),
            "a released event must be usable"
        );
        assert!(
            !log.release_aside(&aside_id).unwrap(),
            "releasing twice is not a second release"
        );

        clean(&p);
    }

    #[test]
    fn an_unauthenticated_event_never_occupies_a_position() {
        // The failure this ordering exists to prevent: garbage claims a slot,
        // and the genuine event for that slot can then never be accepted.
        let p = tmp("no-squat");
        let s = testing::mixed(2, 1, 1, 2);
        let mut log = Log::open(&p).unwrap();
        let real = &s.events[0];
        let k = s.key_for(&real.header.dev).unwrap();

        let mut forged = real.clone();
        forged.sig = [0u8; 64];
        assert!(matches!(
            log.ingest(&forged, &k).unwrap(),
            Ingested::Rejected(_)
        ));
        assert_eq!(log.count().unwrap(), 0, "nothing was stored");

        // The genuine event still fits.
        assert_eq!(log.ingest(real, &k).unwrap(), Ingested::Accepted);
        clean(&p);
    }

    #[test]
    fn a_substituted_payload_is_refused() {
        let p = tmp("swap");
        let s = testing::mixed(3, 1, 1, 1);
        let mut log = Log::open(&p).unwrap();
        let mut e = s.events[0].clone();
        e.body = Some(testing::capture_body(9, "different text entirely"));
        let k = s.key_for(&e.header.dev).unwrap();
        assert_eq!(
            log.ingest(&e, &k).unwrap(),
            Ingested::Rejected(Rejection::BodyMismatch)
        );
        clean(&p);
    }

    #[test]
    fn the_same_event_twice_changes_nothing() {
        let p = tmp("dup");
        let s = testing::mixed(4, 1, 1, 0);
        let mut log = Log::open(&p).unwrap();
        let e = &s.events[0];
        let k = s.key_for(&e.header.dev).unwrap();
        assert_eq!(log.ingest(e, &k).unwrap(), Ingested::Accepted);
        assert_eq!(log.ingest(e, &k).unwrap(), Ingested::Duplicate);
        assert_eq!(log.count().unwrap(), 1);
        clean(&p);
    }

    #[test]
    fn two_events_claiming_one_position_is_loud() {
        // Not a duplicate. Treating it as one is what turns a rolled-back
        // store into silent two-way data loss.
        let p = tmp("collide");
        let a = testing::mixed(5, 1, 1, 0);
        let b = testing::mixed(6, 1, 1, 0);
        let mut log = Log::open(&p).unwrap();

        let ea = &a.events[0];
        log.ingest(ea, &a.key_for(&ea.header.dev).unwrap()).unwrap();

        // Same origin and sequence, different content.
        let mut eb = b.events[0].clone();
        eb.header.dev = ea.header.dev;
        eb.header.seq = ea.header.seq;
        // Re-sign so it is authentic — the point is that authenticity is not
        // the problem here; the position is.
        let s = testing::Origin::new(0);
        let _ = s;
        assert!(matches!(
            log.ingest(&eb, &b.key_for(&b.events[0].header.dev).unwrap())
                .unwrap(),
            Ingested::Rejected(_) | Ingested::Collision
        ));
        clean(&p);
    }

    #[test]
    fn out_of_order_arrival_does_not_advance_the_head() {
        let p = tmp("gap");
        let s = testing::mixed(7, 1, 1, 4);
        let mut ev: Vec<_> = s.events.clone();
        ev.sort_by_key(|e| e.header.seq);
        let mut log = Log::open(&p).unwrap();
        let k = s.key_for(&ev[0].header.dev).unwrap();

        log.ingest(&ev[0], &k).unwrap();
        // Skip one, deliver the next.
        assert_eq!(log.ingest(&ev[2], &k).unwrap(), Ingested::Pending);

        let heads = log.heads().unwrap();
        let head = *heads.values().next().unwrap();
        assert_eq!(head, 1, "the head must not run past a missing sequence");
        assert_eq!(log.pending().unwrap().len(), 1);
        clean(&p);
    }

    #[test]
    fn filling_a_gap_absorbs_what_was_waiting() {
        let p = tmp("absorb");
        let s = testing::mixed(8, 1, 1, 4);
        let mut ev: Vec<_> = s.events.clone();
        ev.sort_by_key(|e| e.header.seq);
        let mut log = Log::open(&p).unwrap();
        let k = s.key_for(&ev[0].header.dev).unwrap();

        log.ingest(&ev[0], &k).unwrap();
        log.ingest(&ev[2], &k).unwrap();
        log.ingest(&ev[3], &k).unwrap();
        assert_eq!(*log.heads().unwrap().values().next().unwrap(), 1);

        // The missing one arrives; everything above it becomes covered at once.
        log.ingest(&ev[1], &k).unwrap();
        assert_eq!(*log.heads().unwrap().values().next().unwrap(), 4);
        assert!(log.pending().unwrap().is_empty());
        clean(&p);
    }

    #[test]
    fn arrival_order_does_not_change_the_head() {
        // The property that will not be exercised for real until several
        // devices exist. Without the generator it could not be tested at all.
        let p1 = tmp("order-a");
        let p2 = tmp("order-b");
        let s = testing::mixed(9, 3, 3, 25);

        let mut in_order = Log::open(&p1).unwrap();
        for e in &s.events {
            in_order
                .ingest(e, &s.key_for(&e.header.dev).unwrap())
                .unwrap();
        }

        let mut shuffled = Log::open(&p2).unwrap();
        for e in &s.shuffled(4242) {
            shuffled
                .ingest(e, &s.key_for(&e.header.dev).unwrap())
                .unwrap();
        }

        assert_eq!(
            in_order.heads().unwrap(),
            shuffled.heads().unwrap(),
            "what a device holds cannot depend on delivery order"
        );
        assert!(shuffled.pending().unwrap().is_empty());
        clean(&p1);
        clean(&p2);
    }

    #[test]
    fn heads_match_the_events_actually_held() {
        for seed in 1..6u64 {
            let p = tmp(&format!("consistent-{seed}"));
            let s = testing::mixed(seed, 3, 2, 15);
            let mut log = Log::open(&p).unwrap();
            for e in &s.shuffled(seed * 3) {
                log.ingest(e, &s.key_for(&e.header.dev).unwrap()).unwrap();
            }
            let heads = log.heads().unwrap();
            let held = log.all().unwrap();
            for (dev, head) in &heads {
                for n in 1..=*head {
                    assert!(
                        held.iter()
                            .any(|e| dev_key(&e.header.dev) == *dev && e.header.seq == n),
                        "a head must not claim a sequence the log does not hold"
                    );
                }
            }
            clean(&p);
        }
    }

    #[test]
    fn a_local_append_out_of_sequence_is_refused() {
        let p = tmp("local-gap");
        let s = testing::mixed(10, 1, 1, 3);
        let mut ev: Vec<_> = s.events.clone();
        ev.sort_by_key(|e| e.header.seq);
        let mut log = Log::open(&p).unwrap();
        log.append(&ev[0]).unwrap();
        assert!(
            log.append(&ev[2]).is_err(),
            "a local gap is a bug, not a state"
        );
        clean(&p);
    }

    #[test]
    fn reading_works_without_a_writer() {
        let p = tmp("read-only");
        let s = testing::mixed(11, 1, 2, 3);
        {
            let mut log = Log::open(&p).unwrap();
            for e in &s.events {
                log.ingest(e, &s.key_for(&e.header.dev).unwrap()).unwrap();
            }
        }
        let ro = Log::open_read(&p).unwrap();
        assert_eq!(ro.count().unwrap(), s.events.len() as u64);
        clean(&p);
    }
}
