//! The projection: the log, arranged for reading.
//!
//! # Disposable on purpose
//!
//! Nothing here is authoritative. Every row is computed from the log and can be
//! computed again, which is what makes this the entire migration strategy: when
//! the shape needs to change, the answer is to rebuild rather than to write a
//! transformation from the old shape to the new one. There is no migration
//! code, and there never will be.
//!
//! The price is that a rebuild takes as long as folding the whole log. The
//! saving is that a decade of accumulated migration steps, each of which has to
//! keep working forever, never exists.
//!
//! # Rebuilds do not overwrite
//!
//! A rebuild writes a **new generation** beside the current one and then moves
//! a pointer. It does not replace the file in place.
//!
//! Renaming a new database over an open one looks equivalent and is not: other
//! processes keep reading the file they already opened, and SQLite's sidecar
//! files are named from the *path*, so a fresh database can end up paired with
//! the previous one's write-ahead log. Generations avoid the question entirely —
//! readers notice the pointer moved and reopen.

use std::path::{Path, PathBuf};

use brain_core::{
    Dev, EventId,
    envelope::Envelope,
    fold::{Held, Projection},
    roster::Roster,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Result, db};

/// Bumped when the shape below changes. A mismatch triggers a rebuild.
pub const PROJECTION_SCHEMA: u32 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL) STRICT;

CREATE TABLE IF NOT EXISTS notes (
  tgt             BLOB PRIMARY KEY,
  slug            TEXT NOT NULL UNIQUE,
  text            TEXT,
  head            BLOB NOT NULL,
  created_wall    INTEGER NOT NULL,
  created_counter INTEGER NOT NULL,
  updated_wall    INTEGER NOT NULL,
  updated_counter INTEGER NOT NULL,
  tz              TEXT,
  pinned          INTEGER NOT NULL DEFAULT 0,
  muted_until     INTEGER,
  redacted_at     INTEGER
) STRICT;

CREATE INDEX IF NOT EXISTS notes_updated ON notes(updated_wall DESC, updated_counter DESC);
CREATE INDEX IF NOT EXISTS notes_created ON notes(created_wall DESC, created_counter DESC);

-- Every version, including ones excluded from the fold. An excluded version's
-- edge still matters: drop it and its successors point at nothing, so a walk
-- terminates early and reports edits made in sequence as independent.
CREATE TABLE IF NOT EXISTS note_versions (
  event_id    BLOB PRIMARY KEY,
  tgt         BLOB NOT NULL,
  prev        BLOB,
  hlc_wall    INTEGER NOT NULL,
  hlc_counter INTEGER NOT NULL,
  kind        INTEGER NOT NULL,
  excluded    INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS nv_tgt  ON note_versions(tgt, hlc_wall, hlc_counter);
CREATE INDEX IF NOT EXISTS nv_prev ON note_versions(prev);

CREATE TABLE IF NOT EXISTS divergences (
  tgt     BLOB NOT NULL,
  current BLOB NOT NULL,
  other   BLOB NOT NULL,
  PRIMARY KEY (tgt, current, other)
) STRICT;

CREATE TABLE IF NOT EXISTS tags (
  tgt BLOB NOT NULL,
  tag TEXT NOT NULL,
  PRIMARY KEY (tgt, tag)
) STRICT;

-- Events stored and verified, but not folded: an unrecognised kind, an
-- unreadable payload, a note that is not present.
CREATE TABLE IF NOT EXISTS held (
  event_id BLOB PRIMARY KEY,
  reason   TEXT NOT NULL
) STRICT;

-- How far the fold has consumed each origin. Per origin, not one number:
-- events arrive out of order, so a single watermark would either skip late
-- arrivals or force a full rebuild every time one turned up.
CREATE TABLE IF NOT EXISTS fold_head (
  dev TEXT PRIMARY KEY,
  seq INTEGER NOT NULL
) STRICT;

-- Search indexes integers; notes are identified by a 32-byte value. Without
-- this mapping a search result cannot be resolved back to a note, and one
-- note's entries cannot be deleted — which redaction requires.
CREATE TABLE IF NOT EXISTS fts_map (
  rowid INTEGER PRIMARY KEY AUTOINCREMENT,
  tgt   BLOB NOT NULL UNIQUE
) STRICT;

CREATE VIRTUAL TABLE IF NOT EXISTS note_fts USING fts5(
  text, content='', contentless_delete=1
);

-- Membership. Derived like everything else here, but read during ingest rather
-- than after it, which is why a rebuild constructs it before anything else and
-- ingest pauses while it is missing.
CREATE TABLE IF NOT EXISTS roster (
  pubkey     BLOB PRIMARY KEY,
  role       TEXT NOT NULL DEFAULT 'full',
  retired    INTEGER NOT NULL DEFAULT 0,
  can_notify INTEGER NOT NULL DEFAULT 0
) STRICT;

-- Membership is keyed by public key; authorship is keyed by origin. A restore
-- keeps the key and mints a new origin, so one key may own several.
CREATE TABLE IF NOT EXISTS roster_origins (
  dev         TEXT PRIMARY KEY,
  pubkey      BLOB NOT NULL REFERENCES roster(pubkey),
  retire_head INTEGER,
  retire_id   BLOB
) STRICT;

-- Membership events that were authentic but conferred nothing. Without this a
-- device that is silently not a member is undiagnosable.
CREATE TABLE IF NOT EXISTS roster_refused (
  event_id BLOB PRIMARY KEY,
  reason   TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS device_caps (
  dev       TEXT NOT NULL,
  job_class TEXT NOT NULL,
  PRIMARY KEY (dev, job_class)
) STRICT;

-- Absent means the compiled-in default. Rows are never seeded, because status
-- has to distinguish \"default\" from \"you set this\", and a seeded row cannot.
CREATE TABLE IF NOT EXISTS settings (
  k      TEXT PRIMARY KEY,
  v      TEXT NOT NULL,
  set_by TEXT,
  hlc    BLOB
) STRICT;

-- Queued destructions. Derived from the redaction events and their
-- cancellations, and therefore here rather than in local state: put it in
-- ops.db and a restore replays the log, marks the notes redacted, and then
-- never destroys anything, because nothing drives the destruction.
CREATE TABLE IF NOT EXISTS redact_queue (
  redact_evt    BLOB PRIMARY KEY,
  tgt           BLOB NOT NULL,
  execute_after INTEGER NOT NULL,
  cancelled     INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE IF NOT EXISTS mutes (
  scope TEXT NOT NULL,
  value TEXT NOT NULL,
  until INTEGER,
  PRIMARY KEY (scope, value)
) STRICT;

-- Authentic events this device cannot yet use: an origin with no roster entry,
-- a second root, a position already held by a different event. They keep their
-- positions and are re-examined whenever membership changes, because the thing
-- that would make them usable often simply has not arrived yet.
CREATE TABLE IF NOT EXISTS quarantine (
  event_id   BLOB PRIMARY KEY,
  dev        TEXT NOT NULL,
  seq        INTEGER NOT NULL,
  reason     TEXT NOT NULL,
  first_seen INTEGER NOT NULL
) STRICT;
";

/// A handle to the projection.
pub struct Store {
    conn: Connection,
    path: PathBuf,
}

impl Store {
    /// Open or create a projection at a generation path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = db::open_write(path)?;
        db::create_schema(&conn, SCHEMA)?;
        let s = Store {
            conn,
            path: path.to_path_buf(),
        };
        if s.schema_version()? == 0 {
            s.set_meta("schema", &PROJECTION_SCHEMA.to_string())?;
        }
        Ok(s)
    }

    /// Open read-only.
    pub fn open_read(path: &Path) -> Result<Self> {
        Ok(Store {
            conn: db::open_read(path)?,
            path: path.to_path_buf(),
        })
    }

    /// Where this generation lives.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The shape this projection was built with.
    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .conn
            .query_row("SELECT v FROM meta WHERE k = 'schema'", [], |r| {
                r.get::<_, String>(0)
            })
            .optional()?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0))
    }

    /// Whether this projection needs rebuilding.
    pub fn is_stale(&self) -> Result<bool> {
        Ok(self.schema_version()? != PROJECTION_SCHEMA)
    }

    fn set_meta(&self, k: &str, v: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (k, v) VALUES (?1, ?2) ON CONFLICT(k) DO UPDATE SET v = ?2",
            params![k, v],
        )?;
        Ok(())
    }

    /// Replace everything with a freshly folded projection.
    pub fn write(&mut self, p: &Projection) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "DELETE FROM notes; DELETE FROM note_versions; DELETE FROM divergences;
             DELETE FROM tags; DELETE FROM held;
             DELETE FROM note_fts; DELETE FROM fts_map;",
        )?;
        write_into(&tx, p)?;
        tx.commit()?;
        self.set_meta("schema", &PROJECTION_SCHEMA.to_string())?;
        Ok(())
    }

    /// Merge a partial fold, replacing only the notes it covers.
    ///
    /// Used for incremental updates: the caller selects every event belonging
    /// to the affected notes, folds those, and hands the result here. Notes the
    /// partial fold does not mention are left alone.
    pub fn merge(&mut self, p: &Projection) -> Result<()> {
        let tx = self.conn.transaction()?;
        for tgt in p.notes.keys() {
            let t = tgt.0.to_vec();
            tx.execute("DELETE FROM notes WHERE tgt = ?1", params![t])?;
            tx.execute("DELETE FROM note_versions WHERE tgt = ?1", params![t])?;
            tx.execute("DELETE FROM divergences WHERE tgt = ?1", params![t])?;
            tx.execute("DELETE FROM tags WHERE tgt = ?1", params![t])?;
            if let Some(row) = fts_row(&tx, tgt)? {
                tx.execute("DELETE FROM note_fts WHERE rowid = ?1", params![row])?;
            }
        }
        write_into(&tx, p)?;
        tx.commit()?;
        Ok(())
    }

    /// How far the fold has consumed each origin.
    pub fn fold_head(&self, dev: &str) -> Result<u64> {
        Ok(self
            .conn
            .query_row(
                "SELECT seq FROM fold_head WHERE dev = ?1",
                params![dev],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0) as u64)
    }

    /// Record how far an origin has been folded.
    pub fn set_fold_head(&self, dev: &str, seq: u64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO fold_head (dev, seq) VALUES (?1, ?2)
             ON CONFLICT(dev) DO UPDATE SET seq = ?2",
            params![dev, seq as i64],
        )?;
        Ok(())
    }

    /// How many notes are held.
    pub fn note_count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM notes", [], |r| r.get::<_, i64>(0))? as u64)
    }

    /// A note's current text, if any.
    pub fn text_of(&self, tgt: &EventId) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT text FROM notes WHERE tgt = ?1",
                params![tgt.0.to_vec()],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    /// A note's name.
    pub fn slug_of(&self, tgt: &EventId) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT slug FROM notes WHERE tgt = ?1",
                params![tgt.0.to_vec()],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
    }

    /// Notes matching a search, most relevant first.
    pub fn search(&self, query: &str) -> Result<Vec<EventId>> {
        let mut st = self.conn.prepare(
            "SELECT m.tgt FROM note_fts f
             JOIN fts_map m ON m.rowid = f.rowid
             WHERE note_fts MATCH ?1
             ORDER BY rank",
        )?;
        let rows = st.query_map(params![query], |r| r.get::<_, Vec<u8>>(0))?;
        let mut out = Vec::new();
        for row in rows {
            let b = row?;
            let mut a = [0u8; 32];
            a.copy_from_slice(&b[..32]);
            out.push(EventId(a));
        }
        Ok(out)
    }

    /// How many unreconciled branches a note currently has.
    ///
    /// Zero for a note with a single line of edits. Two or more when devices
    /// edited without having seen each other, and nothing has since superseded
    /// either side.
    pub fn divergence_count(&self, tgt: &EventId) -> Result<u64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM divergences WHERE tgt = ?1",
            params![tgt.0.to_vec()],
            |r| r.get::<_, i64>(0),
        )? as u64)
    }

    /// Replace the materialised roster.
    ///
    /// Replaced wholesale rather than patched. Membership is small, and the
    /// alternative is a second set of rules for how it changes, sitting beside
    /// the fold that already decides — which is how a device ends up admitted
    /// on one replica and not on another.
    pub fn write_roster(&mut self, r: &Roster) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM roster_origins", [])?;
        tx.execute("DELETE FROM roster_refused", [])?;
        tx.execute("DELETE FROM roster", [])?;
        for k in &r.keys {
            tx.execute(
                "INSERT INTO roster (pubkey) VALUES (?1)",
                params![k.to_vec()],
            )?;
        }
        for (dev, k) in &r.origins {
            tx.execute(
                "INSERT INTO roster_origins (dev, pubkey) VALUES (?1, ?2)",
                params![dev_hex(dev), k.to_vec()],
            )?;
        }
        for (id, why) in &r.refused {
            tx.execute(
                "INSERT INTO roster_refused (event_id, reason) VALUES (?1, ?2)",
                params![id.0.to_vec(), format!("{why:?}")],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The key an origin is entitled to be verified against.
    ///
    /// `None` means this device cannot check the origin's events at all — not
    /// that they are forged. The claim binding the origin to a key may still be
    /// in flight.
    pub fn key_for(&self, dev: &Dev) -> Result<Option<[u8; 32]>> {
        let row: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT pubkey FROM roster_origins WHERE dev = ?1",
                params![dev_hex(dev)],
                |r| r.get(0),
            )
            .optional()?;
        Ok(row.and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok()))
    }

    /// Whether membership has been established.
    ///
    /// False means ingest must wait. Accepting events while this is false and
    /// checking them afterwards is not a weaker guarantee but no guarantee:
    /// by the time the check runs the events hold positions.
    pub fn roster_is_bootstrapped(&self) -> Result<bool> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM roster", [], |r| r.get::<_, i64>(0))?
            > 0)
    }

    /// Set an authentic event aside as currently unusable.
    pub fn quarantine(
        &self,
        id: &EventId,
        dev: &Dev,
        seq: u64,
        reason: &str,
        at: u64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO quarantine (event_id, dev, seq, reason, first_seen)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id.0.to_vec(), dev_hex(dev), seq as i64, reason, at as i64],
        )?;
        Ok(())
    }

    /// Release an event from quarantine, once it can be used.
    pub fn release(&self, id: &EventId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM quarantine WHERE event_id = ?1",
            params![id.0.to_vec()],
        )?;
        Ok(())
    }

    /// Everything currently set aside.
    pub fn quarantined(&self) -> Result<Vec<(EventId, String)>> {
        let mut st = self
            .conn
            .prepare("SELECT event_id, reason FROM quarantine ORDER BY first_seen")?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (b, reason) = row?;
            let mut a = [0u8; 32];
            a.copy_from_slice(&b[..32]);
            out.push((EventId(a), reason));
        }
        Ok(out)
    }

    /// Events stored but not folded, by reason.
    pub fn held_count(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM held", [], |r| r.get::<_, i64>(0))? as u64)
    }
}

fn fts_row(tx: &rusqlite::Transaction<'_>, tgt: &EventId) -> Result<Option<i64>> {
    Ok(tx
        .query_row(
            "SELECT rowid FROM fts_map WHERE tgt = ?1",
            params![tgt.0.to_vec()],
            |r| r.get::<_, i64>(0),
        )
        .optional()?)
}

fn write_into(tx: &rusqlite::Transaction<'_>, p: &Projection) -> Result<()> {
    for (tgt, n) in &p.notes {
        let t = tgt.0.to_vec();
        tx.execute(
            "INSERT INTO notes
               (tgt, slug, text, head, created_wall, created_counter,
                updated_wall, updated_counter, tz, pinned, muted_until, redacted_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![
                t,
                n.slug,
                n.text,
                n.head.0.to_vec(),
                n.created.wall as i64,
                n.created.counter as i64,
                n.updated.wall as i64,
                n.updated.counter as i64,
                n.tz,
                n.pinned as i64,
                n.muted_until.map(|v| v as i64),
                n.redacted_at.map(|h| h.wall as i64),
            ],
        )?;

        for v in &n.versions {
            tx.execute(
                "INSERT OR REPLACE INTO note_versions
                   (event_id, tgt, prev, hlc_wall, hlc_counter, kind, excluded)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    v.id.0.to_vec(),
                    t,
                    v.prev.map(|x| x.0.to_vec()),
                    v.hlc.wall as i64,
                    v.hlc.counter as i64,
                    v.kind.0 as i64,
                    v.excluded as i64,
                ],
            )?;
        }

        for d in &n.divergences {
            tx.execute(
                "INSERT OR IGNORE INTO divergences (tgt, current, other) VALUES (?1,?2,?3)",
                params![t, d.current.0.to_vec(), d.other.0.to_vec()],
            )?;
        }

        for tag in &n.tags {
            tx.execute(
                "INSERT OR IGNORE INTO tags (tgt, tag) VALUES (?1, ?2)",
                params![t, tag],
            )?;
        }

        // Index the *current* text. Indexing the original capture instead would
        // mean nothing added in an edit was ever findable. A redacted note has
        // no text and contributes no entry, which is what makes its content
        // unfindable after destruction.
        if let Some(text) = &n.text {
            tx.execute(
                "INSERT OR IGNORE INTO fts_map (tgt) VALUES (?1)",
                params![t],
            )?;
            let row: i64 = tx.query_row(
                "SELECT rowid FROM fts_map WHERE tgt = ?1",
                params![t],
                |r| r.get(0),
            )?;
            tx.execute(
                "INSERT INTO note_fts (rowid, text) VALUES (?1, ?2)",
                params![row, text],
            )?;
        }
    }

    for (id, reason) in &p.held {
        let r = match reason {
            Held::UnknownKind => "unknown_kind",
            Held::Malformed => "malformed",
            Held::NoTarget => "no_target",
        };
        tx.execute(
            "INSERT OR REPLACE INTO held (event_id, reason) VALUES (?1, ?2)",
            params![id.0.to_vec(), r],
        )?;
    }

    Ok(())
}

/// Path for a given projection generation.
pub fn generation_path(base: &Path, generation: u64) -> PathBuf {
    let dir = base.parent().unwrap_or(Path::new("."));
    dir.join(format!("state.{generation}.db"))
}

/// Fold everything and write a fresh generation.
///
/// Returns the new generation number. The caller moves the pointer once this
/// succeeds, so a rebuild that fails partway leaves the previous generation
/// untouched and still current.
pub fn rebuild(base: &Path, events: &[Envelope], current: u64) -> Result<u64> {
    let next = current + 1;
    let path = generation_path(base, next);
    let _ = std::fs::remove_file(&path);

    let p = brain_core::fold::fold(events);
    let mut store = Store::open(&path)?;
    store.write(&p)?;

    // Record how far each origin has been consumed, so later updates can be
    // incremental rather than starting again.
    let mut per: std::collections::BTreeMap<String, u64> = Default::default();
    for e in events {
        let d = dev_hex(&e.header.dev);
        let cur = per.entry(d).or_insert(0);
        *cur = (*cur).max(e.header.seq);
    }
    for (d, s) in per {
        store.set_fold_head(&d, s)?;
    }
    Ok(next)
}

/// Remove a superseded generation.
pub fn discard_generation(base: &Path, generation: u64) -> Result<()> {
    let p = generation_path(base, generation);
    for suffix in ["", "-wal", "-shm"] {
        let mut q = p.clone().into_os_string();
        q.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(q));
    }
    Ok(())
}

fn dev_hex(d: &Dev) -> String {
    d.0.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::testing;

    fn base(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("brain-proj-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("state.db")
    }

    fn clean(p: &Path) {
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn a_fold_survives_being_written_and_read() {
        let b = base("roundtrip");
        let s = testing::mixed(1, 2, 3, 12);
        let p = brain_core::fold::fold(&s.events);
        let mut store = Store::open(&generation_path(&b, 1)).unwrap();
        store.write(&p).unwrap();

        assert_eq!(store.note_count().unwrap(), p.notes.len() as u64);
        for (tgt, n) in &p.notes {
            assert_eq!(
                store.slug_of(tgt).unwrap().as_deref(),
                Some(n.slug.as_str())
            );
            assert_eq!(store.text_of(tgt).unwrap(), n.text);
        }
        clean(&b);
    }

    #[test]
    fn a_rebuild_writes_a_new_generation_and_leaves_the_old_one() {
        // Never replaces the file in place: other processes keep reading the
        // one they opened, and the sidecar files are named from the path.
        let b = base("generations");
        let s = testing::mixed(2, 2, 2, 8);
        let g1 = rebuild(&b, &s.events, 0).unwrap();
        let g2 = rebuild(&b, &s.events, g1).unwrap();
        assert_eq!((g1, g2), (1, 2));
        assert!(
            generation_path(&b, 1).exists(),
            "the old generation is still readable"
        );
        assert!(generation_path(&b, 2).exists());
        clean(&b);
    }

    #[test]
    fn a_rebuild_reproduces_the_same_state() {
        let b = base("reproduce");
        let s = testing::mixed(3, 3, 3, 20);
        let g = rebuild(&b, &s.events, 0).unwrap();
        let a = Store::open_read(&generation_path(&b, g)).unwrap();
        let n1 = a.note_count().unwrap();

        let g2 = rebuild(&b, &s.events, g).unwrap();
        let c = Store::open_read(&generation_path(&b, g2)).unwrap();
        assert_eq!(n1, c.note_count().unwrap());
        clean(&b);
    }

    #[test]
    fn a_shape_change_is_detected_rather_than_migrated() {
        let b = base("stale");
        let path = generation_path(&b, 1);
        {
            let store = Store::open(&path).unwrap();
            assert!(!store.is_stale().unwrap());
            store.set_meta("schema", "0").unwrap();
        }
        let store = Store::open_read(&path).unwrap();
        assert!(
            store.is_stale().unwrap(),
            "an old shape must be rebuilt, never transformed in place"
        );
        clean(&b);
    }

    #[test]
    fn search_finds_a_note_by_its_text() {
        let b = base("search");
        let mut d = testing::Origin::new(0);
        let e = d.author(
            brain_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "the deploy keeps timing out on staging"),
            1_700_000_000_000,
        );
        let g = rebuild(&b, std::slice::from_ref(&e), 0).unwrap();
        let store = Store::open_read(&generation_path(&b, g)).unwrap();
        let hits = store.search("staging").unwrap();
        assert_eq!(hits, alloc_vec(&[e.header.id()]));
        clean(&b);
    }

    #[test]
    fn search_indexes_the_current_text_not_the_original() {
        // Indexing the capture instead would mean nothing added in an edit was
        // ever findable.
        let b = base("search-edit");
        let mut d = testing::Origin::new(0);
        let cap = d.author(
            brain_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "original wording"),
            1_700_000_000_000,
        );
        let tgt = cap.header.id();
        let edit = d.author(
            brain_core::Kind::EDIT,
            Some(tgt),
            Some(tgt),
            testing::edit_body(2, "replacement wording with pangolin in it"),
            1_700_000_001_000,
        );
        let g = rebuild(&b, &[cap, edit], 0).unwrap();
        let store = Store::open_read(&generation_path(&b, g)).unwrap();
        assert_eq!(store.search("pangolin").unwrap().len(), 1);
        assert!(store.search("original").unwrap().is_empty());
        clean(&b);
    }

    #[test]
    fn a_redacted_note_is_unfindable() {
        use brain_core::body::{RedactBody, RedactScope};
        let b = base("search-redact");
        let mut d = testing::Origin::new(0);
        let cap = d.author(
            brain_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "postgres prod hunter2 distinctive"),
            1_700_000_000_000,
        );
        let tgt = cap.header.id();
        let body = testing::encode(&RedactBody {
            nonce: [0u8; 16],
            scope: RedactScope::WholeNote,
            targets: alloc_vec(&[tgt]),
        });
        let red = d.author(
            brain_core::Kind::REDACT,
            Some(tgt),
            None,
            body,
            1_700_000_002_000,
        );

        let g = rebuild(&b, &[cap, red], 0).unwrap();
        let store = Store::open_read(&generation_path(&b, g)).unwrap();
        assert!(
            store.search("distinctive").unwrap().is_empty(),
            "destroying the payload but leaving the index would keep it searchable"
        );
        assert!(store.text_of(&tgt).unwrap().is_none());
        clean(&b);
    }

    #[test]
    fn held_events_are_recorded() {
        let b = base("held");
        let s = testing::unknown_kind(4);
        let g = rebuild(&b, &s.events, 0).unwrap();
        let store = Store::open_read(&generation_path(&b, g)).unwrap();
        assert!(store.held_count().unwrap() > 0);
        clean(&b);
    }

    #[test]
    fn discarding_a_generation_removes_its_sidecars() {
        let b = base("discard");
        let s = testing::mixed(5, 1, 1, 2);
        let g = rebuild(&b, &s.events, 0).unwrap();
        discard_generation(&b, g).unwrap();
        assert!(!generation_path(&b, g).exists());
        clean(&b);
    }

    fn alloc_vec<T: Clone>(v: &[T]) -> Vec<T> {
        v.to_vec()
    }
}
