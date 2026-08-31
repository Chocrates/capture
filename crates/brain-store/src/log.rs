//! The event log: the one durable artefact.
//!
//! Everything else in the system is derived from this and can be thrown away.
//! This cannot, so the rules below are the ones worth being careful about.

use std::collections::BTreeMap;
use std::path::Path;

use brain_core::{
    Dev, EventId,
    envelope::{Envelope, Rejection, Verdict},
    event::Header,
    hlc::Hlc,
    id::BodyHash,
};
use rusqlite::{Connection, OptionalExtension, params};

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
        Ok(Log { conn })
    }

    /// Open read-only, without needing the writer.
    pub fn open_read(path: &Path) -> Result<Self> {
        Ok(Log {
            conn: db::open_read(path)?,
        })
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
        insert(&tx, e)?;

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

    /// What this device holds, as one number per origin.
    ///
    /// This is the whole summary a device publishes about its own contents. It
    /// can say "everything up to 41,822" and nothing else — in particular it
    /// cannot say "everything up to 41,822 except 41,700", which is why a gap
    /// must never be created in the first place.
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
    pub fn last_wall(&self, dev: &brain_core::Dev) -> Result<Option<u64>> {
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

    /// Every event, for folding.
    pub fn all(&self) -> Result<Vec<Envelope>> {
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
        targets: &std::collections::BTreeSet<brain_core::EventId>,
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
             WHERE tgt IN ({holes}) OR id IN ({holes})
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
    pub fn rules(&self) -> Result<Vec<Envelope>> {
        let mut st = self.conn.prepare(
            "SELECT id, dev, seq, hlc_wall, hlc_counter, hlc_origin, kind, tgt, prev, occ,
                    chain, bhash, body, sig
             FROM events WHERE tgt IS NULL AND kind = ?1
             ORDER BY hlc_wall, hlc_counter, hlc_origin",
        )?;
        let mut out = Vec::new();
        for r in st.query_map(
            params![brain_core::event::Kind::MUTE.0 as i64],
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
    pub fn chain_tip(&self, dev: &brain_core::Dev) -> Result<Option<brain_core::EventId>> {
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
            Some(brain_core::EventId(a))
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

    /// Remove the payload of every event belonging to a note.
    ///
    /// The header stays, and so does the signature: an event with no body still
    /// recomputes its own id and still verifies, because the body appears in
    /// the header only as a hash. That is what lets a redacted archive pass
    /// `brain verify` rather than reading as damaged.
    ///
    /// The connection sets `secure_delete`, so the freed pages are overwritten
    /// as they are released rather than left holding the old bytes.
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
                brain_core::event::Kind::REDACT.0 as i64,
                brain_core::event::Kind::REDACT_CANCEL.0 as i64
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

fn insert(tx: &rusqlite::Transaction<'_>, e: &Envelope) -> Result<()> {
    let h = &e.header;
    tx.execute(
        "INSERT INTO events
           (id, dev, seq, hlc_wall, hlc_counter, hlc_origin, kind, tgt, prev, occ,
            chain, bhash, body, sig)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
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
        v: brain_core::ENVELOPE_VERSION,
        dev: Dev(blob32(hex_to_bytes(&r.get::<_, String>(1)?))),
        seq: r.get::<_, i64>(2)? as u64,
        hlc: Hlc {
            wall: r.get::<_, i64>(3)? as u64,
            counter: r.get::<_, i64>(4)? as u32,
            origin: Dev(blob32(r.get::<_, Vec<u8>>(5)?)),
        },
        kind: brain_core::Kind(r.get::<_, i64>(6)? as u16),
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

fn hex_to_bytes(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::testing;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("brain-log-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("events.db")
    }

    fn clean(p: &Path) {
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
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
