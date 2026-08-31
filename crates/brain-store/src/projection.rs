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
    fold::{Held, Names, Projection},
    roster::Roster,
};
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Result, db};

/// Bumped when the shape below changes. A mismatch triggers a rebuild.
///
/// Version 4 adds `notes.parent`, which every existing note has to be re-read
/// for — a migration would have had nothing to fill it from.
///
/// Version 5 adds `promotion_at`, so the sweep can tell that a note's text has
/// changed since it was last read. Without it an edit never reparsed, and the
/// fields went stale while the text moved.
///
/// A rebuild rather than a migration: the projection is derived data, and
/// re-folding the log produces it exactly (§1.3). Version 3 adds `mentions`,
/// which needs every existing note re-read for the sigils in its text — a
/// migration could not have filled it in at all.
pub const PROJECTION_SCHEMA: u32 = 5;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL) STRICT;

CREATE TABLE IF NOT EXISTS notes (
  tgt             BLOB PRIMARY KEY,
  slug            TEXT NOT NULL UNIQUE,
  alias           TEXT UNIQUE,
  alias_wall      INTEGER,
  alias_counter   INTEGER,
  text            TEXT,
  head            BLOB NOT NULL,
  created_wall    INTEGER NOT NULL,
  created_counter INTEGER NOT NULL,
  updated_wall    INTEGER NOT NULL,
  updated_counter INTEGER NOT NULL,
  tz              TEXT,
  pinned          INTEGER NOT NULL DEFAULT 0,
  muted_until     INTEGER,
  redacted_at     INTEGER,
  -- Set at capture by a predicate that deliberately over-flags. Keeps the note
  -- out of exports until someone clears it, because an export directory is
  -- copied offsite where redaction can never reach.
  credential      INTEGER NOT NULL DEFAULT 0
) STRICT;

-- The page a note sits under in the wiki (§3.7's one exception, and it is an
-- exception). Read by the sidebar and by nothing else, so a page under the
-- wrong parent is misfiled in one list and is still in the timeline, in search,
-- on every entity page naming it and behind every link to it.
--
-- # Why this is a table and not a column on `notes`
--
-- It was a column, and that broke every store that already existed. This schema
-- is applied to a projection before its version is checked — it has to be, or
-- there would be nothing to read the version *from* — and `CREATE TABLE IF NOT
-- EXISTS notes` does nothing to a table that is already there. So the column
-- never appeared, the index over it failed, and opening the projection failed
-- with it: which meant the rebuild that would have fixed everything could never
-- run, because opening is how a rebuild starts.
--
-- The rule that follows, and it is a rule: **the schema may add tables, never
-- columns.** Rebuild-not-migrate (§1.3) depends on an old projection still
-- being openable long enough to be found stale, and altering a table it already
-- has takes that away.
CREATE TABLE IF NOT EXISTS parents (
  tgt    BLOB PRIMARY KEY,
  parent BLOB NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS parents_parent ON parents(parent);

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

-- Links written in each note's current text.
--
-- Both halves are stored: the name as it was typed, and what that name meant
-- when it was typed. A reader compares them, and shows the link as broken when
-- they disagree rather than following the name to wherever it points now.
CREATE TABLE IF NOT EXISTS links (
  src      BLOB NOT NULL,
  ord      INTEGER NOT NULL,
  name     TEXT NOT NULL,
  resolved BLOB,
  PRIMARY KEY (src, ord)
) STRICT;

-- Backlinks are read far more often than they are written: every note view
-- asks what points at it.
CREATE INDEX IF NOT EXISTS links_resolved ON links(resolved);

-- What was going on when each note was captured. Displayed on the row and
-- usable as a filter: context is often the recognition cue when the text is
-- not, which is the only reason it is collected.
CREATE TABLE IF NOT EXISTS ambient (
  tgt    BLOB PRIMARY KEY,
  app    TEXT,
  title  TEXT,
  cwd    TEXT,
  repo   TEXT,
  branch TEXT
) STRICT;

CREATE INDEX IF NOT EXISTS ambient_repo   ON ambient(repo);
CREATE INDEX IF NOT EXISTS ambient_branch ON ambient(branch);

-- Who and what a note names: `@sarah`, `^acme-migration` (§4.3).
--
-- Derived from the note's current text, like the full-text index and unlike
-- tags — a tag is added and retracted by its own events, where a mention is
-- part of what the note says and changes only when the text does. A redacted
-- note has no text and so names nobody, which is what keeps a destroyed note's
-- mentions off every entity page.
CREATE TABLE IF NOT EXISTS mentions (
  tgt   BLOB NOT NULL,
  sigil TEXT NOT NULL,
  value TEXT NOT NULL,
  PRIMARY KEY (tgt, sigil, value)
) STRICT;

-- An entity page is this query and nothing else, so it is the one that has to
-- be fast.
CREATE INDEX IF NOT EXISTS mentions_value ON mentions(sigil, value);

CREATE TABLE IF NOT EXISTS tags (
  tgt BLOB NOT NULL,
  tag TEXT NOT NULL,
  PRIMARY KEY (tgt, tag)
) STRICT;

-- Which tag events still stand behind each tag. An untag retracts specific
-- additions rather than a word, so removing a tag needs to know exactly which
-- events put it there — and must not touch an addition made on a device this
-- one has never heard from.
CREATE TABLE IF NOT EXISTS tag_adds (
  tgt      BLOB NOT NULL,
  tag      TEXT NOT NULL,
  event_id BLOB NOT NULL,
  PRIMARY KEY (tgt, event_id)
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
  k           TEXT PRIMARY KEY,
  -- Nullable on purpose: a user who explicitly restores the default has made a
  -- choice, and that is a different fact from never having chosen. Only the
  -- second should follow a future change to what the default is.
  v           TEXT,
  set_by      TEXT NOT NULL,
  hlc_wall    INTEGER NOT NULL,
  hlc_counter INTEGER NOT NULL
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

-- What each note was recognised as, decoded once at write time.
--
-- The fields are kept whole as CBOR *and* the parts a query needs are stored
-- beside them. That duplication is deliberate: an agenda asking what is on
-- Tuesday must not decode every promotion in the store to find out, and
-- §3.1.4 wants a descriptor rendered when the fields change rather than on
-- every read.
CREATE TABLE IF NOT EXISTS promotions (
  tgt        BLOB PRIMARY KEY,
  -- note | task | event | series
  kind       TEXT NOT NULL,
  fields     BLOB NOT NULL,
  -- The one-line rendering of the fields, regenerated with them.
  descriptor TEXT NOT NULL,
  -- A human correction, which no reparse may displace.
  pinned     INTEGER NOT NULL DEFAULT 0,
  grammar    INTEGER,
  confidence INTEGER,
  -- Denormalised so the calendar and the agenda are indexed queries.
  start_ms   INTEGER,
  end_ms     INTEGER,
  start_date TEXT,
  end_date   TEXT,
  due        TEXT,
  done       INTEGER
) STRICT;

-- When a promotion was written, which is how the sweep knows the text has
-- changed under it.
--
-- A table rather than two more columns on `promotions`, for the reason written
-- above `parents`: the schema is applied to an existing projection before its
-- version is read, and `CREATE TABLE IF NOT EXISTS` does nothing to a table
-- that is already there. Columns added this way never appear, and anything
-- selecting one fails on every store that predates it.
CREATE TABLE IF NOT EXISTS promotion_at (
  tgt     BLOB PRIMARY KEY,
  wall    INTEGER NOT NULL,
  counter INTEGER NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS promotions_start ON promotions(start_ms);
CREATE INDEX IF NOT EXISTS promotions_due   ON promotions(due);
CREATE INDEX IF NOT EXISTS promotions_kind  ON promotions(kind);

-- Expanded occurrences of every recurring series, over a rolling horizon.
--
-- Derived data, never truth. The rule is the truth (§3.4) and this is what a
-- calendar draws; a rebuild discards every row here and recomputes it, and an
-- exhausted horizon costs an expansion rather than losing anything.
--
-- `occ` is the occurrence's *name* — the instant the rule would have produced
-- before any move — and `start_ms` is where it is actually shown. They differ
-- exactly when something moved it, which is why an exception written on another
-- device can still find the occurrence it is about.
CREATE TABLE IF NOT EXISTS occurrences (
  tgt       BLOB NOT NULL,
  occ       INTEGER NOT NULL,
  start_ms  INTEGER NOT NULL,
  end_ms    INTEGER,
  all_day   INTEGER NOT NULL DEFAULT 0,
  -- gap | ambiguous, when a DST transition moved this occurrence. Shown rather
  -- than applied silently: a time that moves on its own with no explanation is
  -- what makes someone stop trusting the whole system (§4.4).
  shift     TEXT,
  cancelled INTEGER NOT NULL DEFAULT 0,
  done      INTEGER NOT NULL DEFAULT 0,
  title     TEXT,
  location  TEXT,
  notes     TEXT,
  PRIMARY KEY (tgt, occ)
) STRICT;

CREATE INDEX IF NOT EXISTS occurrences_start ON occurrences(start_ms);

-- What review has done with each note.
--
-- Counts of engagement and nothing about memory. §2.7 removed the graded
-- outcome deliberately, and there is no column here for one: `engaged` and
-- `deferred` are what somebody did, which is a different fact from how well
-- they remembered and the only one this design will record.
CREATE TABLE IF NOT EXISTS review (
  tgt        BLOB PRIMARY KEY,
  last_shown INTEGER,
  engaged    INTEGER NOT NULL DEFAULT 0,
  deferred   INTEGER NOT NULL DEFAULT 0
) STRICT;

-- If-then plans (§2.4).
--
-- Retracted ones are kept. A plan somebody gave up on is a fact about what they
-- tried, and the log does not forget things merely because they stopped
-- applying.
CREATE TABLE IF NOT EXISTS intents (
  id        BLOB PRIMARY KEY,
  cue_type  TEXT NOT NULL,
  -- What the cue matches on: an instant for `time`, a branch name for
  -- `git_branch`, an application for `app_focus`. Null where the cue needs no
  -- argument, as `device_wake` does not.
  cue_match TEXT,
  cue_at    INTEGER,
  then_do   TEXT NOT NULL,
  made_at   INTEGER NOT NULL,
  retracted INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE INDEX IF NOT EXISTS intents_type ON intents(cue_type, retracted);

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

/// One note, as a view needs it.
///
/// Every state a note can be in is representable here rather than being an
/// One expanded occurrence.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct OccurrenceRow {
    /// Its name: the instant the rule would have produced, before any move.
    pub occ: i64,
    /// Where it is actually shown. Differs from `occ` exactly when moved.
    pub start_ms: i64,
    /// When it ends, where an exception said so.
    pub end_ms: Option<i64>,
    /// Whether this is a day rather than a moment.
    pub all_day: bool,
    /// `gap` or `ambiguous`, when a DST transition moved it.
    pub shift: Option<String>,
    /// Skipped. Still drawn if it was also moved.
    pub cancelled: bool,
    /// Completed, for a recurring obligation.
    pub done: bool,
    /// A title for this occurrence only.
    pub title: Option<String>,
    /// A location for this occurrence only.
    pub location: Option<String>,
    /// Notes on this occurrence only.
    pub notes: Option<String>,
}

/// The columns a `NoteRow` is read from, aliased to `n`.
const NOTE_COLUMNS: &str = "n.tgt, n.slug, n.alias,
     -- Withheld while a redaction is queued against it. §1.1 makes the window
     -- cancellable, and until it elapses the content is *already* unreadable —
     -- ordering the destruction is what withholds it, not the destruction.
     --
     -- Here rather than at each call site: it was written out in two of them
     -- and absent from the rest, so every query that used this constant could
     -- read a note somebody had already asked to destroy.
     CASE WHEN EXISTS (SELECT 1 FROM redact_queue q2
                       WHERE q2.tgt = n.tgt AND q2.cancelled = 0)
          THEN NULL ELSE n.text END,
     n.created_wall, n.created_counter,
     n.updated_wall, n.updated_counter, n.tz, n.pinned, n.muted_until,
     n.redacted_at, n.credential,
     EXISTS (SELECT 1 FROM redact_queue q WHERE q.tgt = n.tgt AND q.cancelled = 0),
     (SELECT p.parent FROM parents p WHERE p.tgt = n.tgt)";

/// What every review tier excludes, and why.
///
/// Muting always wins (§2.6): anything you do not want resurfaced stops being
/// resurfaced, reversibly, without being destroyed. Two deferrals end it —
/// somebody who has declined an item twice has told you, and pressing further
/// is what §2.7 exists to prevent. A note with no readable text has nothing to
/// offer. And a note is not due until its interval has elapsed.
const REVIEW_ELIGIBLE: &str = "n.text IS NOT NULL
       AND n.muted_until IS NULL
       AND n.redacted_at IS NULL
       AND COALESCE(r.deferred, 0) < 2
       AND (r.last_shown IS NULL
            OR ?1 - r.last_shown >= CASE COALESCE(r.engaged, 0)
                 WHEN 0 THEN 86400000
                 WHEN 1 THEN 259200000
                 WHEN 2 THEN 604800000
                 WHEN 3 THEN 1814400000
                 ELSE 5184000000 END)";

fn utc() -> brain_time::Zone {
    brain_time::Zone::new("UTC").expect("UTC is always present")
}

/// A page in the wiki: a note somebody named, and where it sits.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Named {
    pub alias: String,
    pub slug: String,
    /// The slug of the page it sits under, if any.
    pub parent: Option<String>,
}

/// An if-then plan, as the projection stores it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IntentRow {
    /// The event that created it.
    pub id: Vec<u8>,
    /// `time`, `recurring`, `app_focus`, `git_branch`, `device_wake`, …
    pub cue_type: String,
    /// What it matches on, where the cue takes an argument.
    pub cue_match: Option<String>,
    /// When it fires, for a time cue.
    pub cue_at: Option<i64>,
    /// What to do about it.
    pub then_do: String,
}

/// A promotion, as the projection stores it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PromotionRow {
    /// `note`, `task`, `event` or `series`.
    pub kind: String,
    /// The fields, decoded.
    pub as_: brain_core::body::Promoted,
    /// The one-line rendering, generated when the fields were written.
    pub descriptor: String,
    /// Whether a person authored it, and therefore whether a reparse may
    /// replace it.
    pub pinned: bool,
    /// The grammar behind it, where a recognizer produced it.
    pub grammar: Option<u32>,
    /// The confidence it was written with, where a recognizer produced it.
    pub confidence: Option<u32>,
}

/// absence: a note with no readable text is not the same thing as a note that
/// was redacted, and rendering either as a blank row loses the distinction the
/// reader most needs.
#[derive(Clone, Debug)]
pub struct NoteRow {
    pub tgt: EventId,
    pub slug: String,
    /// A chosen name, if one is assigned. Shares one namespace with slugs.
    pub alias: Option<String>,
    /// Absent when the note has been redacted, when a redaction is queued
    /// against it, or when it has no readable text yet.
    pub text: Option<String>,
    pub created: (u64, u32),
    pub updated: (u64, u32),
    /// The zone the capture happened in — the timeline groups by this and not
    /// by the reader's zone, so travel does not misfile a note.
    pub tz: Option<String>,
    pub pinned: bool,
    pub muted_until: Option<u64>,
    pub redacted_at: Option<u64>,
    /// Whether a redaction has been ordered and not yet carried out.
    ///
    /// Distinct from `redacted_at`, which is destruction. Between the two the
    /// content is withheld but still recoverable, and a view has to be able to
    /// say which of those it is looking at: "queued for destruction" is a thing
    /// the user can still cancel, and "destroyed" is not.
    pub redaction_queued: bool,
    /// Kept out of exports until cleared. See the fold's `credential` field.
    pub credential: bool,
    pub tags: Vec<String>,
    /// The page this one sits under, if somebody filed it there.
    pub parent: Option<EventId>,
    /// How many unreconciled branches the note has.
    pub divergences: u64,
}

impl NoteRow {
    /// Whether the note's content was destroyed.
    pub fn is_redacted(&self) -> bool {
        self.redacted_at.is_some()
    }
}

/// A link as written, alongside what its name means now.
#[derive(Clone, Debug)]
pub struct LinkRow {
    /// The name the author typed.
    pub name: String,
    /// What that name meant when it was written.
    pub resolved: Option<EventId>,
    /// What the same name denotes now. Often the same note; sometimes another
    /// one, if an alias moved; sometimes nothing.
    pub now: Option<EventId>,
    /// The canonical name of the note it was written against, if that note is
    /// still here.
    pub target_slug: Option<String>,
}

impl LinkRow {
    /// Whether the name no longer denotes what it did when it was written.
    ///
    /// Compared by **target**, not by string. A note reached through an alias
    /// has a name that never equalled its slug, and comparing text would call
    /// every such link broken; a name that has been reassigned to a different
    /// note compares equal to nothing and would be missed the other way.
    ///
    /// A link that never resolved is not broken. A forward reference that was
    /// never fulfilled is a different thing from one that was and then moved,
    /// and conflating them would make the marker meaningless.
    pub fn is_broken(&self) -> bool {
        match self.resolved {
            Some(then) => self.now != Some(then),
            None => false,
        }
    }
}

/// A note that links to another.
#[derive(Clone, Debug)]
pub struct Backlink {
    pub tgt: EventId,
    pub slug: String,
    /// The name that note used to refer here.
    pub name: String,
}

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
    /// Write a folded projection.
    ///
    /// `now` is taken as a value rather than read, because it decides the
    /// occurrence horizon and nothing that decides what a store contains may
    /// depend on when it happened to run.
    pub fn write(&mut self, p: &Projection, now: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "DELETE FROM settings;
             DELETE FROM notes; DELETE FROM note_versions; DELETE FROM divergences;
             DELETE FROM ambient; DELETE FROM tags; DELETE FROM tag_adds; DELETE FROM links; DELETE FROM mentions; DELETE FROM parents; DELETE FROM promotion_at; DELETE FROM held;
             DELETE FROM note_fts; DELETE FROM fts_map; DELETE FROM promotions;
             DELETE FROM occurrences; DELETE FROM review; DELETE FROM intents;",
        )?;
        write_into(&tx, p, now)?;
        tx.commit()?;
        self.set_meta("schema", &PROJECTION_SCHEMA.to_string())?;
        Ok(())
    }

    /// Merge a partial fold, replacing only the notes it covers.
    ///
    /// Used for incremental updates: the caller selects every event belonging
    /// to the affected notes, folds those, and hands the result here. Notes the
    /// partial fold does not mention are left alone.
    pub fn merge(&mut self, p: &Projection, now: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        for tgt in p.notes.keys() {
            let t = tgt.0.to_vec();
            tx.execute("DELETE FROM notes WHERE tgt = ?1", params![t])?;
            tx.execute("DELETE FROM note_versions WHERE tgt = ?1", params![t])?;
            tx.execute("DELETE FROM divergences WHERE tgt = ?1", params![t])?;
            tx.execute("DELETE FROM ambient WHERE tgt = ?1", params![t])?;
            tx.execute("DELETE FROM tags WHERE tgt = ?1", params![t])?;
            tx.execute("DELETE FROM mentions WHERE tgt = ?1", params![t])?;
            tx.execute("DELETE FROM parents WHERE tgt = ?1", params![t])?;
            tx.execute("DELETE FROM promotion_at WHERE tgt = ?1", params![t])?;
            tx.execute("DELETE FROM tag_adds WHERE tgt = ?1", params![t])?;
            tx.execute("DELETE FROM links WHERE src = ?1", params![t])?;
            tx.execute("DELETE FROM redact_queue WHERE tgt = ?1", params![t])?;
            tx.execute("DELETE FROM promotions WHERE tgt = ?1", params![t])?;
            tx.execute("DELETE FROM occurrences WHERE tgt = ?1", params![t])?;
            tx.execute("DELETE FROM review WHERE tgt = ?1", params![t])?;
            if let Some(row) = fts_row(&tx, tgt)? {
                tx.execute("DELETE FROM note_fts WHERE rowid = ?1", params![row])?;
            }
        }
        write_into(&tx, p, now)?;
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
                "SELECT CASE WHEN EXISTS (SELECT 1 FROM redact_queue q
                                          WHERE q.tgt = notes.tgt AND q.cancelled = 0)
                             THEN NULL ELSE text END
                 FROM notes WHERE tgt = ?1",
                params![tgt.0.to_vec()],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten())
    }

    /// The text as stored, withholding nothing.
    ///
    /// Exists for exactly one caller: the purge, which needs the content in
    /// order to take the recovery snapshot §1.1 promises — and which only ever
    /// runs against a note whose text every other reader is being refused.
    /// Reading a note's content through this is a decision, which is why it is
    /// not the ordinary accessor and why the name says so.
    pub fn text_for_purge(&self, tgt: &EventId) -> Result<Option<String>> {
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

    /// What a note was recognised as, if anything.
    ///
    /// Returns the decoded fields, the rendered descriptor, and whether a
    /// person pinned it. A caller wanting only the descriptor should take it
    /// from here rather than re-rendering: two surfaces rendering the same
    /// fields differently would be a third version of the truth.
    pub fn promotion_of(&self, tgt: &EventId) -> Result<Option<PromotionRow>> {
        let row = self
            .conn
            .query_row(
                "SELECT kind, fields, descriptor, pinned, grammar, confidence
                 FROM promotions WHERE tgt = ?1",
                params![tgt.0.to_vec()],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)? != 0,
                        r.get::<_, Option<i64>>(4)?,
                        r.get::<_, Option<i64>>(5)?,
                    ))
                },
            )
            .optional()?;

        let Some((kind, fields, descriptor, pinned, grammar, confidence)) = row else {
            return Ok(None);
        };

        Ok(Some(PromotionRow {
            kind,
            as_: minicbor::decode(&fields).map_err(|e| {
                crate::Error::Integrity(format!("a stored promotion could not be read: {e}"))
            })?,
            descriptor,
            pinned,
            grammar: grammar.map(|g| g as u32),
            confidence: confidence.map(|c| c as u32),
        }))
    }

    /// Every note promoted to a given type, newest first.
    pub fn promoted(&self, kind: &str, limit: usize) -> Result<Vec<(EventId, String)>> {
        let mut st = self.conn.prepare(
            "SELECT p.tgt, p.descriptor
             FROM promotions p JOIN notes n ON n.tgt = p.tgt
             WHERE p.kind = ?1
             ORDER BY n.created_wall DESC
             LIMIT ?2",
        )?;
        let rows = st.query_map(params![kind, limit as i64], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (id, d) = r?;
            if let Some(id) = id_from(id) {
                out.push((id, d));
            }
        }
        Ok(out)
    }

    /// Occurrences of a series, in time order.
    ///
    /// Cancelled ones are included and flagged rather than filtered out here: a
    /// view decides whether to draw them, and a moved occurrence is drawn even
    /// when something also cancelled it (§6).
    pub fn occurrences_of(&self, tgt: &EventId) -> Result<Vec<OccurrenceRow>> {
        let mut st = self.conn.prepare(
            "SELECT occ, start_ms, end_ms, all_day, shift, cancelled, done, title, location, notes
             FROM occurrences WHERE tgt = ?1 ORDER BY start_ms",
        )?;
        let rows = st.query_map(params![tgt.0.to_vec()], |r| {
            Ok(OccurrenceRow {
                occ: r.get(0)?,
                start_ms: r.get(1)?,
                end_ms: r.get(2)?,
                all_day: r.get::<_, i64>(3)? != 0,
                shift: r.get(4)?,
                cancelled: r.get::<_, i64>(5)? != 0,
                done: r.get::<_, i64>(6)? != 0,
                title: r.get(7)?,
                location: r.get(8)?,
                notes: r.get(9)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Everything occurring inside a window, across every series.
    ///
    /// The query a calendar or an agenda runs. Answered from expanded rows and
    /// stored fields, never by reading a note's text.
    pub fn occurring_between(&self, from: i64, to: i64) -> Result<Vec<(EventId, OccurrenceRow)>> {
        let mut st = self.conn.prepare(
            "SELECT tgt, occ, start_ms, end_ms, all_day, shift, cancelled, done,
                    title, location, notes
             FROM occurrences
             WHERE start_ms >= ?1 AND start_ms < ?2
             ORDER BY start_ms",
        )?;
        let rows = st.query_map(params![from, to], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                OccurrenceRow {
                    occ: r.get(1)?,
                    start_ms: r.get(2)?,
                    end_ms: r.get(3)?,
                    all_day: r.get::<_, i64>(4)? != 0,
                    shift: r.get(5)?,
                    cancelled: r.get::<_, i64>(6)? != 0,
                    done: r.get::<_, i64>(7)? != 0,
                    title: r.get(8)?,
                    location: r.get(9)?,
                    notes: r.get(10)?,
                },
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (id, o) = r?;
            if let Some(id) = id_from(id) {
                out.push((id, o));
            }
        }
        Ok(out)
    }

    /// Everything happening in a window, recurring or not.
    ///
    /// The query an agenda or a calendar runs. Two sources, because a one-off
    /// event and an instance of a series are different things in the log and
    /// the same thing to somebody looking at a day: occurrences come from the
    /// expansion, and one-off promotions come from their own stored fields.
    ///
    /// A one-off is reported as an occurrence naming itself, which is true —
    /// there is exactly one, and it is where the rule that produced it put it.
    pub fn happening_between(&self, from: i64, to: i64) -> Result<Vec<(EventId, OccurrenceRow)>> {
        let mut out = self.occurring_between(from, to)?;

        // What the expansion already accounts for, so the fallback below does
        // not say it a second time.
        //
        // A one-off event lives in `promotions` and gains a row in
        // `occurrences` only once somebody writes an exception against it —
        // ticking it done is the usual way. From that moment it was in both,
        // and the agenda drew it twice: once carrying the completion and once
        // from here with `done: false` hardcoded, so the tick appeared not to
        // have taken. Only the events somebody had actually interacted with
        // doubled, which is why no test caught it — they all ticked series.
        let seen: std::collections::BTreeSet<(EventId, i64)> =
            out.iter().map(|(id, o)| (*id, o.occ)).collect();

        let mut st = self.conn.prepare(
            "SELECT tgt, start_ms, end_ms FROM promotions
             WHERE kind = 'event' AND start_ms IS NOT NULL
               AND start_ms >= ?1 AND start_ms < ?2",
        )?;
        let timed = st.query_map(params![from, to], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<i64>>(2)?,
            ))
        })?;
        for r in timed {
            let (id, start, end) = r?;
            if let Some(id) = id_from(id) {
                if seen.contains(&(id, start)) {
                    continue;
                }
                out.push((
                    id,
                    OccurrenceRow {
                        occ: start,
                        start_ms: start,
                        end_ms: end,
                        all_day: false,
                        shift: None,
                        cancelled: false,
                        done: false,
                        title: None,
                        location: None,
                        notes: None,
                    },
                ));
            }
        }

        out.sort_by_key(|(_, o)| o.start_ms);
        Ok(out)
    }

    /// All-day entries covering a date.
    ///
    /// Separate from the timed query because they are floating dates: they have
    /// no instant to compare and no position on an hour axis (§3.2, §4.4).
    /// Comparing them as text is exact, because `YYYY-MM-DD` sorts as it reads.
    pub fn all_day_on(&self, date: &str) -> Result<Vec<EventId>> {
        let mut st = self.conn.prepare(
            "SELECT tgt FROM promotions
             WHERE kind = 'event' AND start_date IS NOT NULL
               AND start_date <= ?1 AND end_date >= ?1",
        )?;
        let rows = st.query_map(params![date], |r| r.get::<_, Vec<u8>>(0))?;
        let mut out = Vec::new();
        for r in rows {
            if let Some(id) = id_from(r?) {
                out.push(id);
            }
        }
        Ok(out)
    }

    /// Tasks due on a day, still open.
    ///
    /// The agenda is "what is due and what occurs" (§4.4), and until this
    /// existed it was only the second half: a task due today existed, was
    /// listed by `brain tasks`, and was invisible on the day it mattered. That
    /// is the same failure shape as an intent that is registered and never
    /// fires — the system holds the fact and never says it.
    ///
    /// Due dates are floating and compared as text, which is exactly right for
    /// them: "due Friday" is Friday wherever you are, and converting it to an
    /// instant to compare would reintroduce the midnight §3.6 refuses.
    ///
    /// Finished ones come back too, flagged, and only on their own day. A
    /// surface that made a task vanish the instant it was ticked would leave no
    /// way to undo a mistaken tick except by hunting for the note — where an
    /// occurrence stays put with a mark against it and un-ticks in one gesture.
    /// §6 rates a state that cannot be reversed as the failure worth avoiding,
    /// and a mistaken tick is the ordinary case rather than the exotic one.
    /// They do not follow you forward: tomorrow's agenda has no reason to
    /// mention what was finished today.
    pub fn tasks_due_on(&self, date: &str) -> Result<Vec<(EventId, bool)>> {
        let mut st = self.conn.prepare(
            "SELECT tgt, COALESCE(done, 0) FROM promotions
             WHERE kind = 'task' AND due = ?1
             ORDER BY COALESCE(done, 0) ASC",
        )?;
        let rows = st.query_map(params![date], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)? != 0))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (id, done) = r?;
            if let Some(id) = id_from(id) {
                out.push((id, done));
            }
        }
        Ok(out)
    }

    /// Open tasks whose due date has passed.
    ///
    /// Overdue rather than lapsed, and the difference is §3.6's: a task carries
    /// forward by its nature — it is still open — where a missed occurrence of
    /// a series lapses and nothing accumulates.
    pub fn tasks_overdue(&self, today: &str) -> Result<Vec<EventId>> {
        let mut st = self.conn.prepare(
            "SELECT tgt FROM promotions
             WHERE kind = 'task' AND due IS NOT NULL AND due < ?1
               AND COALESCE(done, 0) = 0
             ORDER BY due ASC",
        )?;
        let rows = st.query_map(params![today], |r| r.get::<_, Vec<u8>>(0))?;
        let mut out = Vec::new();
        for r in rows {
            if let Some(id) = id_from(r?) {
                out.push(id);
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------ review ----
    //
    // The three selection tiers of §2.8, in its order. Each excludes the same
    // things for the same reasons, which is why they share `REVIEW_ELIGIBLE`:
    // a muted note is not resurfaced (§2.6), a note declined twice stops being
    // offered rather than being pressed, a note with no readable text has
    // nothing to resurface, and a note whose interval has not elapsed is not
    // due.

    /// Notes carrying a forward marker — "revisit Q2", "check back in a month".
    ///
    /// The first tier, and the strongest signal there is: somebody said, in the
    /// note itself, that this was worth coming back to.
    pub fn review_forward(&self, now: i64, limit: usize) -> Result<Vec<NoteRow>> {
        let sql = format!(
            "SELECT {NOTE_COLUMNS}
             FROM notes n
             JOIN promotions p ON p.tgt = n.tgt
             LEFT JOIN review r ON r.tgt = n.tgt
             WHERE {REVIEW_ELIGIBLE}
               AND ((p.start_ms IS NOT NULL AND p.start_ms > ?1)
                 OR (p.start_date IS NOT NULL AND p.start_date > ?3))
             ORDER BY n.created_wall ASC
             LIMIT ?2"
        );
        // The only tier that compares a date, so the only one given one.
        let today = format!("{}", brain_time::civil_date(now, utc()));
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params![now, limit as i64, today], note_row)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Notes never engaged with since capture, oldest first.
    pub fn review_never_engaged(&self, now: i64, limit: usize) -> Result<Vec<NoteRow>> {
        let sql = format!(
            "SELECT {NOTE_COLUMNS}
             FROM notes n
             LEFT JOIN review r ON r.tgt = n.tgt
             WHERE {REVIEW_ELIGIBLE}
               AND COALESCE(r.engaged, 0) = 0
             ORDER BY n.created_wall ASC
             LIMIT ?2"
        );
        self.review_query(&sql, now, limit)
    }

    /// Notes captured in the context you are in now.
    ///
    /// The recognition cue is frequently the situation rather than the text
    /// (§2.3), so a note captured in this repository is a candidate while you
    /// are in it.
    pub fn review_by_context(&self, now: i64, limit: usize) -> Result<Vec<NoteRow>> {
        let sql = format!(
            "SELECT {NOTE_COLUMNS}
             FROM notes n
             LEFT JOIN review r ON r.tgt = n.tgt
             WHERE {REVIEW_ELIGIBLE}
             ORDER BY n.created_wall ASC
             LIMIT ?2"
        );
        self.review_query(&sql, now, limit)
    }

    fn review_query(&self, sql: &str, now: i64, limit: usize) -> Result<Vec<NoteRow>> {
        let mut st = self.conn.prepare(sql)?;
        let rows = st.query_map(params![now, limit as i64], note_row)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Every if-then plan still standing.
    pub fn intents(&self) -> Result<Vec<IntentRow>> {
        let mut st = self.conn.prepare(
            "SELECT id, cue_type, cue_match, cue_at, then_do FROM intents
             WHERE retracted = 0 ORDER BY made_at",
        )?;
        let rows = st.query_map([], |r| {
            Ok(IntentRow {
                id: r.get::<_, Vec<u8>>(0)?,
                cue_type: r.get(1)?,
                cue_match: r.get(2)?,
                cue_at: r.get(3)?,
                then_do: r.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Instances that have been and gone without being completed.
    ///
    /// "Lapsed" and "owed" are different things and §3.6 keeps them apart. By
    /// default a missed instance lapses and nothing accumulates: miss Tuesday's
    /// walk and there is no debt on Wednesday, because the habit literature is
    /// explicit that a single missed day costs almost nothing, and a system
    /// that manufactures a backlog is representing something that is not true.
    ///
    /// The exception is real and had to exist. A missed dose, an unpaid
    /// invoice, a prescription refill — for those, Wednesday carries a genuine
    /// debt, and silently dropping it is worse than the todo-app failure the
    /// rule was written to fix. Those series set `carries_forward`, and
    /// [`Store::outstanding`] is what still owes.
    pub fn lapsed(&self, series: &EventId, before: i64) -> Result<Vec<OccurrenceRow>> {
        Ok(self
            .occurrences_of(series)?
            .into_iter()
            .filter(|o| o.start_ms < before && !o.done && !o.cancelled)
            .collect())
    }

    /// Everything still owed, across every series that carries forward.
    ///
    /// Only those. A series without the flag has no outstanding anything — its
    /// missed instances lapsed, which is a fact recorded and not a task pending.
    pub fn outstanding(&self, before: i64) -> Result<Vec<(EventId, OccurrenceRow)>> {
        let mut out = Vec::new();
        for (tgt, _) in self.promoted("series", 500)? {
            let carries = match self.promotion_of(&tgt)? {
                Some(p) => match p.as_ {
                    brain_core::body::Promoted::Series(s) => s.carries_forward,
                    _ => false,
                },
                None => false,
            };
            if !carries {
                continue;
            }
            for o in self.lapsed(&tgt, before)? {
                out.push((tgt, o));
            }
        }
        out.sort_by_key(|(_, o)| o.start_ms);
        Ok(out)
    }

    /// Series that keep lapsing and do not carry forward.
    ///
    /// §3.6 asks for one prompt, in review, rather than an inference: a series
    /// that has lapsed repeatedly *might* be one that genuinely accumulates,
    /// and the recognizer must not decide that from phrasing. Asking once is
    /// the difference between noticing and guessing.
    pub fn repeatedly_lapsed(&self, before: i64, threshold: usize) -> Result<Vec<EventId>> {
        let mut out = Vec::new();
        for (tgt, _) in self.promoted("series", 500)? {
            let carries = match self.promotion_of(&tgt)? {
                Some(p) => match p.as_ {
                    brain_core::body::Promoted::Series(s) => s.carries_forward,
                    _ => false,
                },
                None => false,
            };
            if carries {
                continue;
            }
            if self.lapsed(&tgt, before)?.len() >= threshold {
                out.push(tgt);
            }
        }
        Ok(out)
    }

    /// Notes captured while a series was happening.
    ///
    /// The whole of activation on one device (§3.9). A recurring meeting is a
    /// set of windows in time, and the notes taken during previous instances of
    /// it are the ones worth having in front of you at the next one — which is
    /// §0's argument from the other end: when encoding is the weak link, you
    /// frequently cannot recall the note but can recall the situation.
    ///
    /// Derived by overlap rather than by association. Nothing asks the user to
    /// file a note against a meeting, and no event records that it belongs to
    /// one: the capture already carries when it happened, the series already
    /// says when it was on, and the answer falls out of the two. §3.9 allows
    /// for one event kind here and none turned out to be needed, which is worth
    /// preferring — an association somebody has to maintain is one that goes
    /// stale.
    ///
    /// `before` bounds it to instances that have already happened; a meeting
    /// activates on what was said at previous ones, not on what has not
    /// occurred yet.
    pub fn captured_during(
        &self,
        series: &EventId,
        before: i64,
        limit: usize,
    ) -> Result<Vec<NoteRow>> {
        // A window per occurrence. An occurrence with no end lasts an hour,
        // which is what a meeting means when nobody said otherwise — and
        // guessing longer would sweep in notes from after it broke up.
        const ASSUMED_MS: i64 = 60 * 60 * 1000;

        let windows: Vec<(i64, i64)> = self
            .occurrences_of(series)?
            .into_iter()
            .filter(|o| !o.cancelled && o.start_ms < before)
            .map(|o| (o.start_ms, o.end_ms.unwrap_or(o.start_ms + ASSUMED_MS)))
            .collect();

        if windows.is_empty() {
            return Ok(Vec::new());
        }

        let clauses: Vec<String> = windows
            .iter()
            .map(|(a, b)| format!("(n.created_wall >= {a} AND n.created_wall < {b})"))
            .collect();

        let sql = format!(
            "SELECT {NOTE_COLUMNS}
             FROM notes n
             WHERE n.text IS NOT NULL
               AND n.tgt != ?1
               AND ({})
             ORDER BY n.created_wall DESC
             LIMIT ?2",
            clauses.join(" OR ")
        );

        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params![series.0.to_vec(), limit as i64], note_row)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Notes that need reading, or reading again.
    ///
    /// Either nothing has ever parsed them, or what did was an older grammar. A
    /// pinned promotion is never returned: it is somebody's correction, and the
    /// point of the pin is that no later parse displaces it.
    ///
    /// An indexed query rather than a fold. The sweep folded the whole log to
    /// answer this until the projection learned to store a promotion, which was
    /// fine for hundreds of notes and would not have stayed fine.
    pub fn needing_parse(&self, grammar: u32, limit: usize) -> Result<Vec<NoteRow>> {
        let sql = format!(
            "SELECT {NOTE_COLUMNS}
             FROM notes n
             LEFT JOIN promotions p ON p.tgt = n.tgt
             LEFT JOIN promotion_at pa ON pa.tgt = n.tgt
             WHERE n.text IS NOT NULL
               AND (p.tgt IS NULL
                    OR (p.pinned = 0
                        AND ((p.grammar IS NOT NULL AND p.grammar < ?1)
                             -- Or the text has moved since it was read. The
                             -- same predicate `needs_parse` applies in the
                             -- fold, and it has to be the same or the queue
                             -- and the decision disagree about what is stale.
                             OR pa.wall IS NULL
                             OR (pa.wall, pa.counter) < (n.updated_wall, n.updated_counter))))
             ORDER BY n.created_wall DESC
             LIMIT ?2"
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params![grammar as i64, limit as i64], note_row)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
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
    ///
    /// Muted notes are left out. That is what muting is for: the note stays in
    /// the timeline, where it can be seen in its place, and stops appearing in
    /// the surfaces that put things in front of you. A mute that only greyed
    /// something out would not be worth reaching for, and the whole point is
    /// that suppressing something has to be *cheaper* than destroying it.
    ///
    /// A lapsed mute suppresses nothing, so the expiry is compared here rather
    /// than baked into the row.
    pub fn search(&self, query: &str, now: u64) -> Result<Vec<EventId>> {
        let mut st = self.conn.prepare(
            "SELECT m.tgt FROM note_fts f
             JOIN fts_map m ON m.rowid = f.rowid
             JOIN notes n ON n.tgt = m.tgt
             WHERE note_fts MATCH ?1
               AND (n.muted_until IS NULL
                    OR (n.muted_until != 0 AND n.muted_until <= ?2))
             ORDER BY rank",
        )?;
        let rows = st.query_map(params![query, now as i64], |r| r.get::<_, Vec<u8>>(0))?;
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

    /// Every name the projection knows, with the moment it started existing.
    ///
    /// Feeds an incremental fold, which sees only the events of the notes that
    /// changed and would otherwise resolve every link to nothing.
    /// The names among these that the projection knows.
    ///
    /// Bounded on purpose. An incremental fold asks its seed
    /// two questions — what a `[[link]]` in these events points at, and whether
    /// an alias being assigned is already taken — so it needs those names and
    /// no others. Reading every name in the archive to answer them made a write
    /// cost more as the archive grew.
    pub fn names_matching(&self, wanted: &std::collections::BTreeSet<String>) -> Result<Names> {
        let mut out = Names::new();
        if wanted.is_empty() {
            return Ok(out);
        }

        let holes = std::iter::repeat_n("?", wanted.len())
            .collect::<Vec<_>>()
            .join(",");
        let list: Vec<&String> = wanted.iter().collect();

        // Slugs and aliases both, because they share one namespace and a link
        // may be written against either. Each carries the moment it started
        // denoting its note, so a link written earlier does not resolve
        // through it.
        for sql in [
            format!(
                "SELECT slug, tgt, created_wall, created_counter FROM notes
                 WHERE slug IN ({holes})"
            ),
            format!(
                "SELECT alias, tgt, alias_wall, alias_counter FROM notes
                 WHERE alias IN ({holes})"
            ),
        ] {
            let mut st = self.conn.prepare(&sql)?;
            let args: Vec<&dyn rusqlite::ToSql> =
                list.iter().map(|v| *v as &dyn rusqlite::ToSql).collect();
            let rows = st.query_map(args.as_slice(), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    id_from(r.get::<_, Vec<u8>>(1)?).unwrap_or(EventId([0; 32])),
                    r.get::<_, i64>(2)? as u64,
                    r.get::<_, i64>(3)? as u32,
                ))
            })?;
            for row in rows {
                let (name, tgt, wall, counter) = row?;
                out.insert(
                    name,
                    (
                        tgt,
                        brain_core::hlc::Hlc {
                            wall,
                            counter,
                            origin: Dev([0; 32]),
                        },
                    ),
                );
            }
        }
        Ok(out)
    }

    /// Everything a view needs about one note, in one read.
    pub fn note(&self, tgt: &EventId) -> Result<Option<NoteRow>> {
        let row = self
            .conn
            .query_row(
                &format!("SELECT {NOTE_COLUMNS} FROM notes n WHERE n.tgt = ?1"),
                params![tgt.0.to_vec()],
                note_row,
            )
            .optional()?;
        let Some(mut n) = row else { return Ok(None) };
        n.tags = self.tags_of(tgt)?;
        n.divergences = self.divergence_count(tgt)?;
        Ok(Some(n))
    }

    /// Resolve a name to a note.
    ///
    /// One namespace. A name is a slug or it is nothing — aliases join the same
    /// namespace when they arrive, which is what stops an alias shadowing
    /// another note's canonical name.
    pub fn resolve(&self, name: &str) -> Result<Option<EventId>> {
        let row: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT tgt FROM notes WHERE slug = ?1 OR alias = ?1",
                params![name],
                |r| r.get(0),
            )
            .optional()?;
        Ok(row.and_then(id_from))
    }

    /// Every note naming a person or a project.
    ///
    /// The whole of an entity page (§4.3): a filtered timeline of everything
    /// mentioning it. There is no entity record to create and nothing to
    /// maintain — a page exists for every name the moment it is written, which
    /// is what makes this usable on day one rather than after months of
    /// curation.
    ///
    /// Muted notes are excluded, for the same reason they are excluded from
    /// search: §2.6 makes muting the cheap way to stop something resurfacing,
    /// and a page that kept showing it would defeat that.
    pub fn mentioning(&self, sigil: char, value: &str, now: u64) -> Result<Vec<NoteRow>> {
        let sql = format!(
            "SELECT {NOTE_COLUMNS}
             FROM notes n JOIN mentions m ON m.tgt = n.tgt
             WHERE m.sigil = ?1 AND m.value = ?2
               AND (n.muted_until IS NULL
                    OR (n.muted_until != 0 AND n.muted_until <= ?3))
             ORDER BY n.created_wall DESC"
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params![sigil.to_string(), value, now as i64], note_row)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Every note carrying a tag.
    ///
    /// The third sigil, and the one that works differently underneath. A tag is
    /// an OR-Set with its own events (§3.7) — added and retracted after the
    /// fact, and often by enrichment rather than by hand — where a mention is
    /// part of the text. To a reader following one they are the same gesture,
    /// which is why the surfaces treat them alike and only this layer does not.
    ///
    /// Muted notes are excluded, as they are from mentions and search: §2.6
    /// makes muting the cheap way to stop something resurfacing.
    pub fn tagged(&self, tag: &str, now: u64) -> Result<Vec<NoteRow>> {
        // Both sources, and the note appears once. A `#tag` written in a
        // capture and a tag added afterwards are different facts — one is part
        // of what the note says, the other is an OR-Set membership that can be
        // retracted — and a page that showed only one of them would answer a
        // narrower question than the reader asked. `#reading` typed into a
        // capture reached nothing at all before this.
        let sql = format!(
            "SELECT {NOTE_COLUMNS}
             FROM notes n
             WHERE (EXISTS (SELECT 1 FROM tags t WHERE t.tgt = n.tgt AND t.tag = ?1)
                 OR EXISTS (SELECT 1 FROM mentions m
                            WHERE m.tgt = n.tgt AND m.sigil = '#' AND m.value = ?1))
               AND (n.muted_until IS NULL
                    OR (n.muted_until != 0 AND n.muted_until <= ?2))
             ORDER BY n.created_wall DESC"
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params![tag, now as i64], note_row)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Notes given a name by hand.
    ///
    /// The wiki's pages, and the reason the list is aliases rather than every
    /// note: an alias is the one act in this system that says "this note is a
    /// thing I will come back to" (§4.3). Everything else is a capture, and a
    /// sidebar listing all of them would be the timeline with worse ordering.
    pub fn named(&self) -> Result<Vec<Named>> {
        let mut st = self.conn.prepare(
            "SELECT n.alias, n.slug, up.slug FROM notes n
             LEFT JOIN parents pa ON pa.tgt = n.tgt
             LEFT JOIN notes up ON up.tgt = pa.parent
             WHERE n.alias IS NOT NULL AND n.redacted_at IS NULL
             ORDER BY n.alias",
        )?;
        let rows = st.query_map([], |r| {
            Ok(Named {
                alias: r.get::<_, String>(0)?,
                slug: r.get::<_, String>(1)?,
                // The parent by slug rather than by id: the page is addressed
                // by name everywhere else, and a tree keyed by raw bytes would
                // need translating at every use.
                //
                // A parent that is not itself a named page comes back as its
                // slug all the same, so the branch is still drawn — filing a
                // page under an unnamed note is allowed, and hiding the result
                // would look like the move had not worked.
                parent: r.get::<_, Option<String>>(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Every distinct person, project or tag the notes name.
    ///
    /// Ordered by how often each is mentioned, then alphabetically. Frequency
    /// first because a sidebar is read top-down and the name you want is
    /// usually one you use — and because alphabetical alone puts whoever is
    /// called Aaron permanently above everyone you actually work with.
    pub fn distinct_mentions(&self, sigil: char) -> Result<Vec<(String, u32)>> {
        // Tags come from two places, so `#` unions them. The other two sigils
        // exist only in the text and the union is a no-op for them, but writing
        // it once keeps the three consistent rather than special-casing.
        let mut st = self.conn.prepare(
            "SELECT value, COUNT(DISTINCT tgt) AS n FROM (
               SELECT m.value AS value, m.tgt AS tgt
                 FROM mentions m JOIN notes n ON n.tgt = m.tgt
                 WHERE m.sigil = ?1 AND n.redacted_at IS NULL AND n.muted_until IS NULL
               UNION
               SELECT t.tag AS value, t.tgt AS tgt
                 FROM tags t JOIN notes n ON n.tgt = t.tgt
                 WHERE ?1 = '#' AND n.redacted_at IS NULL AND n.muted_until IS NULL
             )
             GROUP BY value
             ORDER BY n DESC, value ASC",
        )?;
        let rows = st.query_map(params![sigil.to_string()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u32))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Names beginning with a prefix, for completing a link as it is typed.
    ///
    /// Slugs and aliases together, because they share one namespace and one
    /// uniqueness constraint (§4.3) — a completion list that offered only slugs
    /// would hide exactly the names somebody chose for being memorable.
    ///
    /// Redacted notes are excluded. A destroyed note still has a slug, and
    /// offering it would invite writing a link to content that is gone; §1.1
    /// keeps the tombstone visible in the timeline, which is where an absence
    /// is explained rather than where a new reference to it is created.
    ///
    /// Ordered by how recently captured, so the note you are thinking of is
    /// usually the first offer: linking is overwhelmingly to something recent.
    pub fn names_like(&self, prefix: &str, limit: usize) -> Result<Vec<String>> {
        // Escaped, because a name may contain the wildcards. A slug is
        // kebab-case and cannot, but an alias is whatever somebody typed, and a
        // `%` in one would otherwise match every note in the store.
        let pattern = format!(
            "{}%",
            prefix
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut st = self.conn.prepare(
            "SELECT name FROM (
               SELECT slug AS name, created_wall FROM notes
                 WHERE slug LIKE ?1 ESCAPE '\\' AND redacted_at IS NULL
               UNION
               SELECT alias AS name, created_wall FROM notes
                 WHERE alias LIKE ?1 ESCAPE '\\' AND redacted_at IS NULL
             )
             ORDER BY created_wall DESC
             LIMIT ?2",
        )?;
        let rows = st.query_map(params![pattern, limit as i64], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Notes newest first.
    ///
    /// Ordered by **capture** time, not by last change. A view that reshuffled
    /// whenever something was edited would destroy the spatial memory that makes
    /// a timeline findable — and the design leans on that memory rather than on
    /// the user remembering where they filed things.
    pub fn timeline(&self, limit: usize) -> Result<Vec<NoteRow>> {
        let sql = format!(
            "SELECT {NOTE_COLUMNS}
             FROM notes n
             ORDER BY n.created_wall DESC, n.created_counter DESC
             LIMIT ?1"
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params![limit as i64], note_row)?;
        let mut out = Vec::new();
        for r in rows {
            let mut n = r?;
            n.tags = self.tags_of(&n.tgt)?;
            n.divergences = self.divergence_count(&n.tgt)?;
            out.push(n);
        }
        Ok(out)
    }

    /// A note's tags.
    pub fn tags_of(&self, tgt: &EventId) -> Result<Vec<String>> {
        let mut st = self
            .conn
            .prepare("SELECT tag FROM tags WHERE tgt = ?1 ORDER BY tag")?;
        let rows = st.query_map(params![tgt.0.to_vec()], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// When redactions were issued, and how many targets each named.
    ///
    /// Feeds the hourly ceiling. Derived from the log like everything else
    /// here, so a restored device rebuilds the history rather than starting
    /// with a clean allowance — which is what a device that had just been
    /// compromised would otherwise get by being restored.
    pub fn redaction_history(&self) -> Result<Vec<(u64, usize)>> {
        let mut st = self.conn.prepare(
            "SELECT v.hlc_wall, COUNT(*) FROM note_versions v
             WHERE v.kind = ?1 GROUP BY v.event_id, v.hlc_wall",
        )?;
        let rows = st.query_map(params![brain_core::event::Kind::REDACT.0 as i64], |r| {
            Ok((r.get::<_, i64>(0)? as u64, r.get::<_, i64>(1)? as usize))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Redactions waiting for their window to elapse.
    ///
    /// Kept here rather than in local state, and that placement is the whole
    /// point: it is a function of the redaction events, so a restored device
    /// rebuilds it by replaying the log. Local state would be wiped by a
    /// re-fold, after which the device would report every redaction complete
    /// while holding every plaintext it was told to destroy.
    pub fn redact_queue(&self) -> Result<Vec<(EventId, EventId, u64, bool)>> {
        let mut st = self.conn.prepare(
            "SELECT redact_evt, tgt, execute_after, cancelled FROM redact_queue
             ORDER BY execute_after",
        )?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, i64>(2)? as u64,
                r.get::<_, i64>(3)? != 0,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (a, b, at, cancelled) = row?;
            if let (Some(a), Some(b)) = (id_from(a), id_from(b)) {
                out.push((a, b, at, cancelled));
            }
        }
        Ok(out)
    }

    /// Remove every derived copy of a note's content.
    ///
    /// The row itself stays: a note that stops existing on redaction would take
    /// its links, its place in the timeline and any reference to it with it,
    /// and the archive would silently lose an entry rather than its content.
    /// What goes is the text, the search index entry, and the version rows that
    /// could reconstruct either.
    pub fn purge_note(&mut self, tgt: &EventId, at: u64) -> Result<()> {
        let t = tgt.0.to_vec();
        let tx = self.conn.transaction()?;

        // The search index first. It is the one place that answers queries with
        // content, so leaving it for last would keep the text findable through
        // every other step.
        if let Some(row) = fts_row(&tx, tgt)? {
            tx.execute("DELETE FROM note_fts WHERE rowid = ?1", params![row])?;
            tx.execute("DELETE FROM fts_map WHERE tgt = ?1", params![t])?;
        }

        // `redacted_at` is set here as well as the text being cleared. A row
        // with no text and no destruction time is a different thing — a note
        // whose content has not arrived yet — and a reader shown "no readable
        // text" where "content destroyed" was true has been told the wrong
        // story about their own archive.
        tx.execute(
            "UPDATE notes SET text = NULL, slug = ?2, redacted_at = ?3 WHERE tgt = ?1",
            params![t, brain_core::slug::derive_redacted(tgt).text, at as i64],
        )?;
        tx.execute("DELETE FROM links WHERE src = ?1", params![t])?;
        tx.commit()?;
        Ok(())
    }

    /// What was going on when a note was captured.
    pub fn ambient_of(&self, tgt: &EventId) -> Result<Vec<(String, String)>> {
        let row = self
            .conn
            .query_row(
                "SELECT app, cwd, repo, branch FROM ambient WHERE tgt = ?1",
                params![tgt.0.to_vec()],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;
        let Some((app, cwd, repo, branch)) = row else {
            return Ok(Vec::new());
        };
        // Absent fields are left out rather than shown empty: "not observed" and
        // "observed to be nothing" are different, and a strip full of blanks
        // teaches the reader to ignore it.
        let mut out = Vec::new();
        for (k, v) in [
            ("app", app),
            ("cwd", cwd),
            ("repo", repo),
            ("branch", branch),
        ] {
            if let Some(v) = v.filter(|v| !v.is_empty()) {
                out.push((k.to_string(), v));
            }
        }
        Ok(out)
    }

    /// Notes sharing an ambient value.
    ///
    /// Context is frequently the recognition cue when the text is not, which is
    /// the reason it is collected at all — so it has to be usable as a filter
    /// rather than only displayed.
    pub fn by_context(&self, field: &str, value: &str) -> Result<Vec<NoteRow>> {
        // The column is chosen from a fixed set rather than interpolated. A
        // field name arriving from the interface is untrusted input, and the
        // one place it could do damage is here.
        let column = match field {
            "cwd" => "cwd",
            "repo" => "repo",
            "branch" => "branch",
            other => {
                return Err(crate::Error::Usage(format!(
                    "'{other}' is not a context field"
                )));
            }
        };

        let sql = format!(
            "SELECT {NOTE_COLUMNS}
             FROM notes n JOIN ambient a ON a.tgt = n.tgt
             WHERE a.{column} = ?1
             ORDER BY n.created_wall DESC"
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params![value], note_row)?;
        let mut out = Vec::new();
        for r in rows {
            let mut n = r?;
            n.tags = self.tags_of(&n.tgt)?;
            n.divergences = self.divergence_count(&n.tgt)?;
            out.push(n);
        }
        Ok(out)
    }

    /// Notes the credential predicate flagged, which exports leave out.
    pub fn credential_flagged(&self) -> Result<Vec<(EventId, String)>> {
        let mut st = self
            .conn
            .prepare("SELECT tgt, slug FROM notes WHERE credential = 1 ORDER BY slug")?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (id, slug) = r?;
            if let Some(id) = id_from(id) {
                out.push((id, slug));
            }
        }
        Ok(out)
    }

    /// Preferences that were actually chosen, with who chose them and when.
    ///
    /// A key that is absent has never been chosen. That absence is the whole
    /// point: it is what lets status say "default" rather than reporting a
    /// value with no indication of where it came from.
    /// One setting's value, if it has ever been chosen.
    ///
    /// `None` means nobody has decided, which is a different fact from a value
    /// that happens to be empty — and it is the difference §8.9 exists to keep.
    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT v FROM settings WHERE k = ?1", params![key], |r| {
                r.get::<_, Option<String>>(0)
            })
            .optional()?
            .flatten())
    }

    pub fn settings(&self) -> Result<Vec<(String, Option<String>, String)>> {
        let mut st = self
            .conn
            .prepare("SELECT k, v, set_by FROM settings ORDER BY k")?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// The `tag` events that put a tag on a note.
    ///
    /// An untag names these rather than naming the tag. Tags are an add-wins
    /// set: a tag added on a device this one has not heard from must survive a
    /// removal that never saw it, which is only expressible by retracting the
    /// specific additions that were observed.
    pub fn tag_events(&self, tgt: &EventId, tag: &str) -> Result<Vec<EventId>> {
        let mut st = self
            .conn
            .prepare("SELECT event_id FROM tag_adds WHERE tgt = ?1 AND tag = ?2")?;
        let rows = st.query_map(params![tgt.0.to_vec(), tag], |r| r.get::<_, Vec<u8>>(0))?;
        let mut out = Vec::new();
        for r in rows {
            if let Some(id) = id_from(r?) {
                out.push(id);
            }
        }
        Ok(out)
    }

    /// The competing versions of a note that nothing has superseded.
    pub fn divergent_versions(&self, tgt: &EventId) -> Result<Vec<EventId>> {
        let mut st = self
            .conn
            .prepare("SELECT other FROM divergences WHERE tgt = ?1")?;
        let rows = st.query_map(params![tgt.0.to_vec()], |r| r.get::<_, Vec<u8>>(0))?;
        let mut out = Vec::new();
        for r in rows {
            if let Some(id) = id_from(r?) {
                out.push(id);
            }
        }
        Ok(out)
    }

    /// Links written in a note, in the order they appear in its text.
    pub fn links_from(&self, tgt: &EventId) -> Result<Vec<LinkRow>> {
        let mut st = self.conn.prepare(
            "SELECT l.name,
                    l.resolved,
                    (SELECT tgt FROM notes WHERE slug = l.name OR alias = l.name),
                    (SELECT slug FROM notes WHERE tgt = l.resolved)
             FROM links l
             WHERE l.src = ?1 ORDER BY l.ord",
        )?;
        let rows = st.query_map(params![tgt.0.to_vec()], link_row)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Notes linking to this one.
    pub fn links_to(&self, tgt: &EventId) -> Result<Vec<Backlink>> {
        let mut st = self.conn.prepare(
            "SELECT n.tgt, n.slug, l.name
             FROM links l JOIN notes n ON n.tgt = l.src
             WHERE l.resolved = ?1 ORDER BY n.created_wall DESC",
        )?;
        let rows = st.query_map(params![tgt.0.to_vec()], |r| {
            Ok(Backlink {
                tgt: id_from(r.get::<_, Vec<u8>>(0)?).unwrap_or(EventId([0; 32])),
                slug: r.get(1)?,
                name: r.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
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
                "INSERT INTO roster (pubkey, retired) VALUES (?1, 0)",
                params![k.to_vec()],
            )?;
        }
        // Retired keys are rows too, not absences. Their origins' past events
        // are still verified against them — those events were legitimately
        // authored — so the key has to be here for `roster_origins` to point
        // at, and "retired" is a property of the row rather than its absence.
        for k in r.retired.keys() {
            tx.execute(
                "INSERT OR IGNORE INTO roster (pubkey, retired) VALUES (?1, 1)",
                params![k.to_vec()],
            )?;
        }
        for (dev, k) in &r.origins {
            let head = r.retired.get(k).and_then(|heads| heads.get(dev));
            tx.execute(
                "INSERT INTO roster_origins (dev, pubkey, retire_head, retire_id)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    dev_hex(dev),
                    k.to_vec(),
                    head.map(|(seq, _)| *seq as i64),
                    head.map(|(_, id)| id.0.to_vec()),
                ],
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

    /// Everything set aside for one reason, with where it sits.
    ///
    /// The position is what makes a reason like `post_retire` re-evaluable: a
    /// retirement reissued with a later head re-admits exactly the events
    /// below it, and deciding that needs the sequence number rather than only
    /// the id.
    pub fn quarantined_at(&self, reason: &str) -> Result<Vec<(EventId, String, u64)>> {
        let mut st = self.conn.prepare(
            "SELECT event_id, dev, seq FROM quarantine WHERE reason = ?1 ORDER BY dev, seq",
        )?;
        let rows = st.query_map(params![reason], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (id, dev, seq) = r?;
            if let Ok(id) = <[u8; 32]>::try_from(id.as_slice()) {
                out.push((EventId(id), dev, seq as u64));
            }
        }
        Ok(out)
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

/// Store what a note was recognised as, decoded once rather than per read.
///
/// The whole `Promoted` is kept as CBOR so nothing is lost, and the parts a
/// query needs are written beside it so the agenda and the calendar are indexed
/// lookups rather than a decode of every promotion in the store. The descriptor
/// is rendered here, with the fields, because §3.1.4 wants it regenerated when
/// they change rather than computed by every surface that draws one.
fn write_promotion(
    tx: &rusqlite::Transaction<'_>,
    tgt: &EventId,
    p: &brain_core::fold::Promotion,
) -> Result<()> {
    use brain_core::body::{Promoted, When};

    let (kind, start_ms, end_ms, start_date, end_date, due, done) = match &p.as_ {
        Promoted::Note => ("note", None, None, None, None, None, None),
        Promoted::Task(t) => (
            "task",
            None,
            None,
            None,
            None,
            t.due.clone(),
            Some(t.done as i64),
        ),
        Promoted::Event(e) => match &e.when {
            When::Timed {
                start_ms, end_ms, ..
            } => ("event", Some(*start_ms), *end_ms, None, None, None, None),
            When::AllDay {
                start_date,
                end_date,
            } => (
                "event",
                None,
                None,
                Some(start_date.clone()),
                Some(end_date.clone()),
                None,
                None,
            ),
        },
        Promoted::Series(_) => ("series", None, None, None, None, None, None),
    };

    let mut fields = Vec::new();
    minicbor::encode(&p.as_, &mut fields)
        .map_err(|e| crate::Error::Integrity(format!("a promotion could not be stored: {e}")))?;

    tx.execute(
        "INSERT OR REPLACE INTO promotion_at (tgt, wall, counter) VALUES (?1,?2,?3)",
        params![tgt.0.to_vec(), p.at.wall as i64, p.at.counter as i64],
    )?;

    tx.execute(
        "INSERT OR REPLACE INTO promotions
           (tgt, kind, fields, descriptor, pinned, grammar, confidence,
            start_ms, end_ms, start_date, end_date, due, done)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![
            tgt.0.to_vec(),
            kind,
            fields,
            crate::descriptor::render(&p.as_),
            p.pinned as i64,
            p.grammar.map(|g| g as i64),
            p.confidence.map(|c| c as i64),
            start_ms,
            end_ms,
            start_date,
            end_date,
            due,
            done,
        ],
    )?;
    Ok(())
}

/// Expand a promoted series into the occurrences a calendar draws.
///
/// Occurrences are projection data: the rule is truth, this is a cache of what
/// the rule produces over the window that anybody is going to look at. A
/// rebuild throws all of it away and recomputes it, which is what makes a
/// timezone database update a re-fold rather than a migration (§3.4).
///
/// Exceptions are applied on top, keyed by the occurrence's original start. One
/// that names an occurrence outside the window — or one the rule no longer
/// produces — is still written, because it is a fact somebody recorded and a
/// completion that vanished when the schedule moved would be a completion
/// silently lost.
fn expand_occurrences(
    tx: &rusqlite::Transaction<'_>,
    tgt: &EventId,
    note: &brain_core::fold::Note,
    promotion: &brain_core::fold::Promotion,
    all: &std::collections::BTreeMap<EventId, brain_core::fold::Note>,
    now: i64,
) -> Result<()> {
    use brain_core::body::{Promoted, When};

    let mut written: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();

    if let Promoted::Series(sf) = &promotion.as_ {
        let (from, to) = brain_time::horizon(now);

        // An all-day series is expanded at local midnight purely so the rows
        // order; `all_day` is what a view reads, and it renders the date.
        let (start_ms, all_day, tz) = match &sf.when {
            When::Timed { start_ms, tz, .. } => (Some(*start_ms), false, tz.clone()),
            When::AllDay { start_date, .. } => (
                brain_time::FloatingDate::parse(start_date).and_then(|d| {
                    let z = brain_time::Zone::new(note.tz.as_deref().unwrap_or("UTC"))?;
                    brain_time::at_local(d, 0, 0, z)
                }),
                true,
                note.tz.clone().unwrap_or_else(|| String::from("UTC")),
            ),
        };

        if let (Some(start_ms), Some(zone)) = (start_ms, brain_time::Zone::new(&tz)) {
            let rule = to_rule(&sf.rule);
            let local = brain_time::local_of(start_ms, zone);

            // Exceptions written against the series this one was split from,
            // for the occurrences this one now covers. §3.5: exceptions survive
            // a split by binding to whichever series covers their original
            // start. Without this, completing next Tuesday and *then* changing
            // the schedule from Tuesday onward would silently lose the
            // completion — it would still be in the log, attached to a series
            // that no longer produces that occurrence.
            let inherited = sf
                .split_from
                .and_then(|parent| all.get(&parent))
                .map(|parent| &parent.occurrences);

            for o in brain_time::expand(&rule, local, zone, from, to) {
                let ex = note
                    .occurrences
                    .get(&o.start)
                    .or_else(|| inherited.and_then(|m| m.get(&o.start)));
                let (shown, end) = match ex.and_then(|e| e.moved_to) {
                    Some((s, e)) => (s, e),
                    None => (o.start, None),
                };
                tx.execute(
                    "INSERT OR REPLACE INTO occurrences
                       (tgt, occ, start_ms, end_ms, all_day, shift, cancelled, done,
                        title, location, notes)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    params![
                        tgt.0.to_vec(),
                        o.start,
                        shown,
                        end,
                        all_day as i64,
                        o.shift.map(|s| match s {
                            brain_time::Shift::Gap => "gap",
                            brain_time::Shift::Ambiguous => "ambiguous",
                        }),
                        ex.map(|e| e.cancelled).unwrap_or(false) as i64,
                        ex.map(|e| e.done).unwrap_or(false) as i64,
                        ex.and_then(|e| e.title.clone()),
                        ex.and_then(|e| e.location.clone()),
                        ex.and_then(|e| e.notes.clone()),
                    ],
                )?;
                written.insert(o.start);
            }
        }
    }

    // Exceptions the expansion did not reach. Kept rather than dropped: the
    // rule can be edited later, and a completion recorded against an occurrence
    // the schedule has since moved is still something somebody did.
    // What an exception falls back to when it did not move the occurrence.
    //
    // A series occurrence the expansion did not reach has no length to recover.
    // A one-off event does: its promotion carries the start and the end, and an
    // exception written against it — a tick is the usual one — must not throw
    // the duration away. It did, so ticking a two-hour workshop shrank it to a
    // mark on the grid, which is what the agenda drew once the duplicate row
    // carrying the real span was removed.
    //
    // Only where the exception names the promotion's own start. If the two have
    // parted company the promotion's span says nothing about this occurrence.
    let one_off_span = match &promotion.as_ {
        brain_core::body::Promoted::Event(e) => match &e.when {
            brain_core::body::When::Timed {
                start_ms, end_ms, ..
            } => Some((*start_ms, *end_ms)),
            _ => None,
        },
        _ => None,
    };

    for (occ, state) in &note.occurrences {
        if written.contains(occ) {
            continue;
        }
        let inherited = one_off_span.filter(|(start, _)| start == occ);
        let (shown, end) = state.moved_to.or(inherited).unwrap_or((*occ, None));
        tx.execute(
            "INSERT OR REPLACE INTO occurrences
               (tgt, occ, start_ms, end_ms, all_day, shift, cancelled, done, title, location, notes)
             VALUES (?1,?2,?3,?4,0,NULL,?5,?6,?7,?8,?9)",
            params![
                tgt.0.to_vec(),
                occ,
                shown,
                end,
                state.cancelled as i64,
                state.done as i64,
                state.title.clone(),
                state.location.clone(),
                state.notes.clone(),
            ],
        )?;
    }

    Ok(())
}

/// The stored rule, as `brain-time` expands it.
fn to_rule(r: &brain_core::body::RuleSpec) -> brain_time::Rule {
    use brain_time::{Freq, Rule, Weekday};

    let freq = match r.freq {
        0 => Freq::Daily,
        1 => Freq::Weekly,
        2 => Freq::Monthly,
        _ => Freq::Yearly,
    };
    let mut rule = Rule::new(freq, r.interval);

    if r.by_day != 0 {
        let all = [
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
            Weekday::Sat,
            Weekday::Sun,
        ];
        let days: Vec<Weekday> = all
            .iter()
            .enumerate()
            .filter(|(i, _)| r.by_day & (1 << i) != 0)
            .map(|(_, d)| *d)
            .collect();
        rule = rule.on_days(&days);
    }
    if !r.by_month_day.is_empty() {
        rule = rule.on_month_days(&r.by_month_day);
    }
    if let Some(n) = r.count {
        rule = rule.count(n);
    }
    if let Some(u) = r.until {
        rule = rule.until(u);
    }
    rule
}

/// Store an if-then plan.
fn write_intent(
    tx: &rusqlite::Transaction<'_>,
    id: &EventId,
    i: &brain_core::fold::Intent,
) -> Result<()> {
    use brain_core::body::Cue;

    let (cue_type, cue_match, cue_at) = match &i.cue {
        Cue::Time(ms) => ("time", None, Some(*ms)),
        Cue::Recurring(_) => ("recurring", None, None),
        Cue::Location { lat_e6, lon_e6, .. } => {
            ("location", Some(format!("{lat_e6},{lon_e6}")), None)
        }
        Cue::AppFocus(app) => ("app_focus", Some(app.clone()), None),
        Cue::CalendarStart(_) => ("calendar_start", None, None),
        Cue::DeviceWake => ("device_wake", None, None),
        Cue::GitBranch(b) => ("git_branch", Some(b.clone()), None),
    };

    tx.execute(
        "INSERT OR REPLACE INTO intents
           (id, cue_type, cue_match, cue_at, then_do, made_at, retracted)
         VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            id.0.to_vec(),
            cue_type,
            cue_match,
            cue_at,
            i.then,
            i.at.wall as i64,
            i.retracted as i64,
        ],
    )?;
    Ok(())
}

fn write_into(tx: &rusqlite::Transaction<'_>, p: &Projection, now: i64) -> Result<()> {
    for (id, i) in &p.intents {
        write_intent(tx, id, i)?;
    }

    for (k, v) in &p.settings {
        tx.execute(
            "INSERT OR REPLACE INTO settings (k, v, set_by, hlc_wall, hlc_counter)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                k,
                v.value,
                dev_hex(&v.set_by),
                v.at.wall as i64,
                v.at.counter as i64
            ],
        )?;
    }

    for (tgt, n) in &p.notes {
        if n.review.last_shown.is_some() || n.review.engaged > 0 || n.review.deferred > 0 {
            tx.execute(
                "INSERT OR REPLACE INTO review (tgt, last_shown, engaged, deferred)
                 VALUES (?1,?2,?3,?4)",
                params![
                    tgt.0.to_vec(),
                    n.review.last_shown.map(|h| h.wall as i64),
                    n.review.engaged as i64,
                    n.review.deferred as i64,
                ],
            )?;
        }

        if let Some(promotion) = &n.promotion {
            write_promotion(tx, tgt, promotion)?;
            expand_occurrences(tx, tgt, n, promotion, &p.notes, now)?;
        }

        if let Some(pending) = &n.redaction_pending {
            tx.execute(
                "INSERT OR REPLACE INTO redact_queue (redact_evt, tgt, execute_after, cancelled)
                 VALUES (?1,?2,?3,0)",
                params![
                    pending.redact.0.to_vec(),
                    tgt.0.to_vec(),
                    pending.execute_after as i64
                ],
            )?;
        }

        let t = tgt.0.to_vec();
        tx.execute(
            "INSERT INTO notes
               (tgt, slug, alias, alias_wall, alias_counter, text, head,
                created_wall, created_counter, updated_wall, updated_counter,
                tz, pinned, muted_until, redacted_at, credential)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            params![
                t,
                n.slug,
                n.alias,
                n.alias_at.map(|h| h.wall as i64),
                n.alias_at.map(|h| h.counter as i64),
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
                n.credential as i64,
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

        for (i, l) in n.links.iter().enumerate() {
            tx.execute(
                "INSERT OR REPLACE INTO links (src, ord, name, resolved) VALUES (?1,?2,?3,?4)",
                params![t, i as i64, l.name, l.resolved.map(|x| x.0.to_vec())],
            )?;
        }

        if let Some(parent) = n.parent {
            tx.execute(
                "INSERT OR REPLACE INTO parents (tgt, parent) VALUES (?1, ?2)",
                params![t, parent.0.to_vec()],
            )?;
        }

        for tag in &n.tags {
            tx.execute(
                "INSERT OR IGNORE INTO tags (tgt, tag) VALUES (?1, ?2)",
                params![t, tag],
            )?;
        }

        // Mentions come from the text a reader can see. `text_for_purge` is
        // deliberately not used here: a note whose content is withheld or
        // destroyed must not go on naming people, or an entity page becomes a
        // way to read what redaction was supposed to have removed.
        if let Some(text) = n.text.as_deref() {
            for (sigil, value) in brain_parse::mentions(text) {
                tx.execute(
                    "INSERT OR IGNORE INTO mentions (tgt, sigil, value) VALUES (?1,?2,?3)",
                    params![t, sigil.to_string(), value],
                )?;
            }
        }

        let a = &n.ambient;
        if a.app.is_some() || a.cwd.is_some() || a.repo.is_some() || a.branch.is_some() {
            tx.execute(
                "INSERT OR REPLACE INTO ambient (tgt, app, title, cwd, repo, branch)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![t, a.app, a.window_title, a.cwd, a.repo, a.branch],
            )?;
        }

        for (tag, add) in &n.tag_adds {
            tx.execute(
                "INSERT OR REPLACE INTO tag_adds (tgt, tag, event_id) VALUES (?1,?2,?3)",
                params![t, tag, add.0.to_vec()],
            )?;
        }

        // Index the *current* text. Indexing the original capture instead would
        // mean nothing added in an edit was ever findable. A redacted note has
        // no text and contributes no entry, which is what makes its content
        // unfindable after destruction — and a note merely *queued* for
        // redaction contributes none either, or the content would stay
        // searchable for the whole cancellation window.
        if let Some(text) = n.text.as_ref().filter(|_| n.redaction_pending.is_none()) {
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
pub fn rebuild(base: &Path, events: &[Envelope], current: u64, now: i64) -> Result<u64> {
    let next = current + 1;
    let path = generation_path(base, next);
    let _ = std::fs::remove_file(&path);

    let p = brain_core::fold::fold(events);
    let mut store = Store::open(&path)?;
    store.write(&p, now)?;

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

fn id_from(b: Vec<u8>) -> Option<EventId> {
    <[u8; 32]>::try_from(b.as_slice()).ok().map(EventId)
}

/// Reads a row selected by [`NOTE_COLUMNS`], in that order.
///
/// Four queries once wrote the column list out by hand instead of using the
/// constant, and adding `parent` to it broke every one of them at run time —
/// "Invalid column index: 14", from a store that was perfectly fine. Nothing
/// caught it, because a hand-written list still compiles.
fn note_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<NoteRow> {
    Ok(NoteRow {
        tgt: id_from(r.get::<_, Vec<u8>>(0)?).unwrap_or(EventId([0; 32])),
        slug: r.get(1)?,
        alias: r.get(2)?,
        text: r.get(3)?,
        created: (r.get::<_, i64>(4)? as u64, r.get::<_, i64>(5)? as u32),
        updated: (r.get::<_, i64>(6)? as u64, r.get::<_, i64>(7)? as u32),
        tz: r.get(8)?,
        pinned: r.get::<_, i64>(9)? != 0,
        muted_until: r.get::<_, Option<i64>>(10)?.map(|v| v as u64),
        redacted_at: r.get::<_, Option<i64>>(11)?.map(|v| v as u64),
        credential: r.get::<_, i64>(12)? != 0,
        redaction_queued: r.get::<_, i64>(13)? != 0,
        parent: r.get::<_, Option<Vec<u8>>>(14)?.and_then(id_from),
        tags: Vec::new(),
        divergences: 0,
    })
}

fn link_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<LinkRow> {
    Ok(LinkRow {
        name: r.get(0)?,
        resolved: r.get::<_, Option<Vec<u8>>>(1)?.and_then(id_from),
        now: r.get::<_, Option<Vec<u8>>>(2)?.and_then(id_from),
        target_slug: r.get(3)?,
    })
}

/// How an origin is spelled in every table that stores one.
///
/// Public because the quarantine's `dev` column is compared against it from
/// outside this module, and re-deriving the spelling at the call site is how
/// two spellings appear.
pub fn dev_hex(d: &Dev) -> String {
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
        store.write(&p, 1_800_000_000_000).unwrap();

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
        let g1 = rebuild(&b, &s.events, 0, 1_800_000_000_000).unwrap();
        let g2 = rebuild(&b, &s.events, g1, 1_800_000_000_000).unwrap();
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
        let g = rebuild(&b, &s.events, 0, 1_800_000_000_000).unwrap();
        let a = Store::open_read(&generation_path(&b, g)).unwrap();
        let n1 = a.note_count().unwrap();

        let g2 = rebuild(&b, &s.events, g, 1_800_000_000_000).unwrap();
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
        let g = rebuild(&b, std::slice::from_ref(&e), 0, 1_800_000_000_000).unwrap();
        let store = Store::open_read(&generation_path(&b, g)).unwrap();
        let hits = store.search("staging", 1_800_000_000_000).unwrap();
        assert_eq!(hits, alloc_vec(&[e.header.id()]));
        clean(&b);
    }

    #[test]
    fn a_queued_redaction_withholds_the_text_and_the_index() {
        // DESIGN.md §1.1: a replica "marks the target redacted and hides it
        // from every view on ingest, immediately — that part is not delayed",
        // because the gap between deciding something must go and it being gone
        // is the window in which it can still leak. The fold keeps the text so
        // the redaction stays cancellable; nothing readable does.
        let b = base("queued-redaction");
        let mut d = testing::Origin::new(0);
        let cap = d.author(
            brain_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "a password nobody should still be able to read"),
            1_700_000_000_000,
        );
        let tgt = cap.header.id();
        let redact = d.author(
            brain_core::Kind::REDACT,
            Some(tgt),
            None,
            testing::encode(&brain_core::body::RedactBody {
                nonce: [0u8; 16],
                scope: brain_core::body::RedactScope::WholeNote,
                targets: vec![tgt],
            }),
            1_700_000_001_000,
        );

        let g = rebuild(&b, &[cap, redact], 0, 1_800_000_000_000).unwrap();
        let store = Store::open_read(&generation_path(&b, g)).unwrap();

        let row = store.note(&tgt).unwrap().expect("the note still exists");
        assert!(
            row.text.is_none(),
            "the content is readable while a redaction is queued"
        );
        assert!(
            row.redaction_queued,
            "and nothing says why it cannot be read"
        );
        assert!(
            row.redacted_at.is_none(),
            "it is queued, not destroyed — cancelling has to still mean something"
        );
        assert_eq!(
            store.search("password", 1_800_000_000_000).unwrap().len(),
            0,
            "the content is still findable by search"
        );
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
        let g = rebuild(&b, &[cap, edit], 0, 1_800_000_000_000).unwrap();
        let store = Store::open_read(&generation_path(&b, g)).unwrap();
        assert_eq!(
            store.search("pangolin", 1_800_000_000_000).unwrap().len(),
            1
        );
        assert!(
            store
                .search("original", 1_800_000_000_000)
                .unwrap()
                .is_empty()
        );
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

        let g = rebuild(&b, &[cap, red], 0, 1_800_000_000_000).unwrap();
        let mut store = Store::open(&generation_path(&b, g)).unwrap();

        // A redaction on its own destroys nothing — that is the cancellation
        // window — but nothing readable survives the order either. §1.1 puts
        // those two facts side by side: the target is hidden from every view on
        // ingest, immediately, while physical destruction waits.
        assert_eq!(
            store
                .search("distinctive", 1_800_000_000_000)
                .unwrap()
                .len(),
            0,
            "the content stayed searchable after destruction was ordered"
        );
        assert!(store.text_of(&tgt).unwrap().is_none());
        assert_eq!(store.redact_queue().unwrap().len(), 1, "and it is queued");

        // Withheld, not destroyed. The body is still there to be recovered,
        // which is the whole difference between this state and the next one.
        assert!(
            store.text_for_purge(&tgt).unwrap().is_some(),
            "ordering a redaction destroyed the content the window exists to save"
        );

        // The purge is what removes it.
        store.purge_note(&tgt, 1_700_000_003_000).unwrap();
        assert!(
            store
                .search("distinctive", 1_800_000_000_000)
                .unwrap()
                .is_empty(),
            "destroying the payload but leaving the index would keep it searchable"
        );
        assert!(store.text_of(&tgt).unwrap().is_none());
        clean(&b);
    }

    #[test]
    fn held_events_are_recorded() {
        let b = base("held");
        let s = testing::unknown_kind(4);
        let g = rebuild(&b, &s.events, 0, 1_800_000_000_000).unwrap();
        let store = Store::open_read(&generation_path(&b, g)).unwrap();
        assert!(store.held_count().unwrap() > 0);
        clean(&b);
    }

    #[test]
    fn discarding_a_generation_removes_its_sidecars() {
        let b = base("discard");
        let s = testing::mixed(5, 1, 1, 2);
        let g = rebuild(&b, &s.events, 0, 1_800_000_000_000).unwrap();
        discard_generation(&b, g).unwrap();
        assert!(!generation_path(&b, g).exists());
        clean(&b);
    }

    fn alloc_vec<T: Clone>(v: &[T]) -> Vec<T> {
        v.to_vec()
    }
}
