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

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use capture_core::{
    Dev, EventId,
    envelope::Envelope,
    fold::{Held, Names, Projection},
    id::{EntId, casefold},
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
/// Version 7 adds `list_items.done`, filled from the `item_done` events that
/// replaced the note-minting first version of ticking. Every existing
/// projection has to be rebuilt for it: the completions are in the log and
/// were never in this table.
///
/// Version 6 adds `list_items`, the expansion of a list capture. Every
/// existing note has to be re-read for it: a capture written before the form
/// existed becomes a list on this rebuild, which is what an additive
/// recognizer (§3.1.3) means and is intended.
///
/// Version 9 re-expands every list. Items are unchanged in shape, but which of
/// them are ticked is now worked out by reconciling each `item_done` against
/// the capture's current items rather than by matching the words — so a
/// completion recorded before this has to be re-read against the list it was
/// written for. There is nothing to migrate: the ticks are in the log and were
/// never in this table.
///
/// Version 8 adds `entities` and `entity_names`, which give the names in
/// `mentions` an identity that survives being renamed (§4.3). Every existing
/// note has to be re-read: the entities are derived from the sigils in the
/// text and have never been stored, so there is nothing for a migration to
/// fill them from. The same rebuild also applies entity-scoped mutes for the
/// first time.
///
/// Version 10 adds `read_by`, which records that the sweep read a note at all
/// — not only that it promoted one. Without it a note the recognizer declines
/// stays in the sweep's queue for ever, and since that queue is bounded at two
/// hundred and ordered newest-first, the sweep re-read the same recent
/// unpromotable notes on every open and never reached anything behind them.
/// Nothing to migrate: the table is a record of what *this build* has looked
/// at, so an empty one is the honest starting state and the first few sweeps
/// after this fill it in.
///
/// Version 11 re-keys `redact_queue` on the pair `(redact_evt, tgt)`. It was
/// keyed by the redaction alone, so every target of a multi-target redaction
/// overwrote the last and only one was queued — one note destroyed, the rest
/// keeping their content while the operation reported success. Nothing is lost
/// in the rebuild and something is regained: the queue is derived from the log,
/// the log has always carried every target, so a redaction that destroyed one
/// of three will destroy the other two as it was always ordered to.
///
/// Version 12 adds `late_cancels`, which records a redaction whose cancellation
/// arrived after it had already been carried out. The fold computed both halves
/// — the content gone, the order withdrawn — and kept neither, so §1.1's
/// promise that such a cancel is "recorded and reported" was half true. It is
/// derived from the log, so the rebuild finds every past instance rather than
/// only new ones.
///
/// Version 13 applies a tag mute to notes that wrote the tag inline. The fold
/// muted only notes in the tag's set, which only `capture tag` writes, so a
/// note that said `#noise` in its text escaped `capture mute #noise`. Which
/// notes are suppressed is stored per note, so the rebuild is what reaches the
/// ones already written.
///
/// A rebuild rather than a migration: the projection is derived data, and
/// re-folding the log produces it exactly (§1.3). Version 3 adds `mentions`,
/// which needs every existing note re-read for the sigils in its text — a
/// migration could not have filled it in at all.
///
/// Version 14 re-derives which occurrences are hidden: a skip made after a
/// move now hides it, where a move used to outrank every skip. The rows an
/// older build wrote say "visible" for those, so they are rebuilt, not trusted.
///
/// Version 15 gives each occurrence of a series with a length that length. The
/// rows an older build wrote have no end, so they are rebuilt.
///
/// Version 16 adds `unclear_days`, derived from text on apply like mentions,
/// and grammar 9 decides it — so a store built by an older grammar is rebuilt
/// rather than asked the wrong questions.
pub const PROJECTION_SCHEMA: u32 = 16;

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
-- A day the text leaves open between two (grammar 9, `capture_parse::UnclearDay`).
--
-- Derived from the words on apply, as mentions are, because the calendar has to
-- ask on the two days the question concerns and the note has no promotion to
-- put it on either. Read against the capture's own moment and zone, so every
-- device derives the same row. `start`/`end` are the bytes to replace.
CREATE TABLE IF NOT EXISTS unclear_days (
  tgt    BLOB PRIMARY KEY,
  first  TEXT NOT NULL,
  second TEXT NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS unclear_days_first  ON unclear_days(first);
CREATE INDEX IF NOT EXISTS unclear_days_second ON unclear_days(second);

CREATE TABLE IF NOT EXISTS mentions (
  tgt   BLOB NOT NULL,
  sigil TEXT NOT NULL,
  value TEXT NOT NULL,
  PRIMARY KEY (tgt, sigil, value)
) STRICT;

-- An entity page is this query and nothing else, so it is the one that has to
-- be fast.
CREATE INDEX IF NOT EXISTS mentions_value ON mentions(sigil, value);

-- Who the names in `mentions` turned out to be (§4.3).
--
-- Separate from `mentions` on purpose, and §1.4 draws it the other way — with
-- `mentions` keyed by `ent_id` rather than by the string. Keeping the
-- extraction keyed by what the text says has two consequences worth the
-- divergence: a merge stays a redirection that touches no mention row, and the
-- fast index above keeps its shape.
--
-- `ent_id` is derived rather than minted, so a row here is a cache of a
-- computation and never the truth. `display` is the spelling somebody chose if
-- they have, and otherwise the earliest one written — which is why this table
-- is recomputed whole rather than patched per note.
CREATE TABLE IF NOT EXISTS entities (
  ent_id      BLOB PRIMARY KEY,
  -- person | project | tag, from the sigil that named it.
  kind        TEXT NOT NULL,
  display     TEXT NOT NULL,
  -- The note that *is* this entity, where one is bound.
  note_tgt    BLOB,
  -- The entity this one turned out to be. Resolution follows it.
  merged_into BLOB
) STRICT;

-- Which written spellings reach which entity.
--
-- More than one row per entity is the ordinary case after a merge: `@sarah` and
-- `@sarah-chen` both land here pointing at whichever survived. Without this an
-- entity page could only ever be the notes carrying one exact string, which is
-- the limitation the whole change exists to remove.
--
-- `name` is case-folded, matching what `EntId::derive` hashes, so a lookup does
-- not have to guess which spelling to try.
CREATE TABLE IF NOT EXISTS entity_names (
  kind   TEXT NOT NULL,
  name   TEXT NOT NULL,
  ent_id BLOB NOT NULL,
  PRIMARY KEY (kind, name)
) STRICT;
CREATE INDEX IF NOT EXISTS entity_names_ent ON entity_names(ent_id);

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
-- A redaction whose cancellation arrived after it had been carried out.
--
-- §1.1 asks for such a cancel to be recorded and reported rather than silently
-- discarded. It was recorded — the event is in the log — and reported nowhere,
-- because the fold knew both halves and kept neither.
--
-- A table rather than a column on `notes`, for the reason written above
-- `parents`: the schema may add tables, never columns.
CREATE TABLE IF NOT EXISTS late_cancels (
  tgt    BLOB PRIMARY KEY,
  redact BLOB NOT NULL
) STRICT;

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
-- Destructions waiting for their window.
--
-- Keyed by the **pair**. It was keyed by `redact_evt` alone, and the fold
-- writes a row per pending note — so every target of one redaction overwrote
-- the last and only one was ever queued. A redaction naming three notes
-- destroyed one and reported success; the other two kept their bodies for ever,
-- because nothing would queue them again.
--
-- Latent while every redaction this build issues names exactly one target, and
-- not latent at all the moment anything emits the shape §1.1 already specifies,
-- where a redact names at most 64 targets.
CREATE TABLE IF NOT EXISTS redact_queue (
  redact_evt    BLOB NOT NULL,
  tgt           BLOB NOT NULL,
  execute_after INTEGER NOT NULL,
  cancelled     INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (redact_evt, tgt)
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

-- That a note was read, and by which grammar — whatever the reading found.
--
-- # Without this the sweep is inert, and that is not a figure of speech
--
-- A promotion is written only when the floor is cleared, and `promote.rs`
-- declines to write one otherwise: an event per ordinary capture, recording
-- that the recognizer found what it usually finds. Both of those are right.
-- Together they left no record of a *negative* result, so `needing_parse` read
-- a missing promotion as meaning not yet read, and every ordinary note stayed
-- in the queue for ever.
--
-- The queue is `ORDER BY created_wall DESC LIMIT 200`. So the sweep re-read the
-- newest two hundred unpromotable notes on every session open and never reached
-- anything behind them. Measured: one dated task, then 250 ordinary notes, then
-- five sweeps — nothing promoted, 251 still queued, and the task that was
-- captured first was never read at all. The bound was meant to spread work
-- across runs; what it actually did was put a horizon on the archive.
--
-- # Why it is here and not in the log
--
-- It is a fact about what a build has looked at, not about the archive, and it
-- does not replicate — another device on another grammar has its own answer.
--
-- A rebuild drops it, which is correct and not merely tolerable: re-reading
-- what the current grammar reads costs one pass over the archive and changes no
-- outcome. It is the same shape as a grammar bump, which the sweep is already
-- written to spread over several runs.
--
-- `wall`/`counter` is the text position it was read at, so an edit makes it
-- unread again — the same comparison `promotion_at` exists for.
CREATE TABLE IF NOT EXISTS read_by (
  tgt     BLOB PRIMARY KEY,
  grammar INTEGER NOT NULL,
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

-- The items a list capture names. §3.6: lists are just tags with a display
-- order.
--
-- Derived data on the same terms as `occurrences`, and for the same reason: one
-- gesture is one event, and what that event *names* is read back out of its
-- text rather than written into the log. A rebuild discards every row here and
-- recomputes it.
--
-- `ord` is the position as written, which is the display order. It is the
-- item's identity for drawing it and is deliberately **not** what completion is
-- stored against: editing a list shifts every position after the edit, so a
-- tick bound to a position would silently move to another item. Completion is a
-- task in the log, matched back by `(tag, item)` — see `promote::tick`.
CREATE TABLE IF NOT EXISTS list_items (
  tgt  BLOB NOT NULL,
  ord  INTEGER NOT NULL,
  tag  TEXT NOT NULL,
  item TEXT NOT NULL,
  -- Filled from the folded note's items, exactly as an occurrence's `done` is
  -- filled from its exceptions. Completion is a fact in the log; this column
  -- is the derived copy a surface reads.
  done INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (tgt, ord)
) STRICT;

-- Reading a list is this query, so it is the one that has to be fast.
CREATE INDEX IF NOT EXISTS list_items_tag ON list_items(tag);

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

/// Which captures have not been read by grammar `?1`, as a `FROM` and a `WHERE`.
///
/// One predicate, two callers: [`Store::needing_parse`] lists them so the sweep
/// can read them, and [`Store::unread_count`] counts them so a surface can say
/// it is behind. Shared rather than restated, because two copies of a staleness
/// predicate drift and the drift is silent in both directions — a count of zero
/// over a real backlog, or a warning that no amount of sweeping clears.
const UNREAD_FROM_WHERE: &str = "FROM notes n
     LEFT JOIN promotions p ON p.tgt = n.tgt
     LEFT JOIN promotion_at pa ON pa.tgt = n.tgt
     LEFT JOIN read_by rb ON rb.tgt = n.tgt
     WHERE n.text IS NOT NULL
       -- A pinned promotion is somebody's correction and no later parse
       -- displaces it (§6), so there is nothing to re-read it for.
       AND (p.tgt IS NULL OR p.pinned = 0)

       -- Not recorded as read by this grammar, at this text.
       --
       -- This clause is what makes the queue finite. It used to be `p.tgt IS
       -- NULL` — a missing promotion meaning not yet read — which is true of
       -- every ordinary capture for ever, because reading one writes nothing.
       -- See `read_by` for what that cost.
       AND NOT (rb.tgt IS NOT NULL
                AND rb.grammar >= ?1
                AND (rb.wall, rb.counter) >= (n.updated_wall, n.updated_counter))

       -- And not carrying a promotion that is already current.
       --
       -- `read_by` is local and a rebuild drops it, while `promotions` is
       -- re-folded from the log — so after a rebuild, and on any device a
       -- promotion reached by sync rather than by its own sweep, a note has a
       -- current promotion and no read record. Treating that as unread swept
       -- the whole archive again and wrote a *duplicate* promote event per
       -- note saying exactly what the existing one said: the log growing by a
       -- note per promotion on every rebuild, and syncing the lot to every
       -- peer. A current promotion is a record that the note was read.
       --
       -- The `promotion_at` comparison is the same one `needs_parse` makes in
       -- the fold, and it has to stay the same or the queue and the decision
       -- disagree about what is stale.
       AND NOT (p.tgt IS NOT NULL
                AND p.grammar IS NOT NULL
                AND p.grammar >= ?1
                AND pa.wall IS NOT NULL
                AND (pa.wall, pa.counter) >= (n.updated_wall, n.updated_counter))";

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

/// What may never produce an alert.
///
/// Named once rather than written into each of the three queries that need it.
/// `NOTE_COLUMNS` above carries the same lesson in its own comment: a
/// hand-written copy of this was present in two call sites and absent from the
/// rest, and every query using the copy could read a note somebody had already
/// asked to destroy.
///
/// Muting wins (§2.6) — it is the escape for a dated capture that was a record
/// rather than a reminder. A note with no readable text has nothing to say. A
/// destroyed one must not have its content read back out of a notification,
/// and neither must one merely *queued* for destruction: §1.1 withholds the
/// content the moment the destruction is ordered, not when it is carried out.
const ALERTABLE: &str = "n.text IS NOT NULL
       AND n.redacted_at IS NULL
       AND NOT EXISTS (SELECT 1 FROM redact_queue q
                       WHERE q.tgt = n.tgt AND q.cancelled = 0)
       AND NOT muted(n.muted_until, ?2)";

/// Which notes may be offered in review, and when one becomes eligible again.
///
/// # What this excludes, and why
///
/// A mute in force wins (§2.6): anything you do not want resurfaced stops
/// being resurfaced, without being destroyed, and a mute with an end stops
/// winning once that end passes. Two deferrals end it — somebody who has
/// declined an item twice has told you, and pressing further is what §2.7
/// exists to prevent. A note with no readable text has nothing to offer. And a
/// note is not due until its interval has elapsed.
///
/// # The ladder is built from `review::interval_ms`, not restated here
///
/// It used to be five milliseconds literals in this string: 86400000, 259200000
/// and so on, which are exactly 1, 3, 7, 21 and 60 days — exactly what
/// `interval_ms` returns, written a second time with no arithmetic to read.
///
/// That is the shape `tools/unwired.py` was written for and names in its own
/// docstring: `discovery::backoff` was "replaced at the call site by an
/// untested copy". Here the tested copy was the orphan and the untested copy
/// was the one that ran, and nothing anywhere compared them. Either could have
/// been edited alone and every test would still have passed.
fn review_eligible() -> String {
    use std::fmt::Write as _;
    let mut ladder = String::from("CASE COALESCE(r.engaged, 0)");
    // The rungs the query names explicitly, and then the one it falls back to.
    // Kept in step with `interval_ms`'s own arms by asking it for each.
    for engaged in 0..4u32 {
        let _ = write!(
            ladder,
            "\n                 WHEN {engaged} THEN {}",
            crate::review::interval_ms(engaged)
        );
    }
    let _ = write!(
        ladder,
        "\n                 ELSE {} END",
        crate::review::interval_ms(4)
    );
    let declined = capture_core::fold::DECLINED_AFTER;
    format!(
        // A mute that has *ended* is not in force. This asked only whether an
        // end existed, so a note muted until Monday was out of review for
        // good — §2.6 names review first among what a mute suppresses
        // "permanently or until a stated date", and the second half never
        // arrived. Unnoticed because no command could set an end, so every
        // mute in a real archive was indefinite, where the two forms agree.
        //
        // Zero is the sentinel for indefinite, so it is excluded before the
        // comparison rather than being read as a date in 1970.
        //
        // `?1` is already the clock, for the backoff ladder below.
        "n.text IS NOT NULL
       AND NOT muted(n.muted_until, ?1)
       AND n.redacted_at IS NULL
       AND COALESCE(r.deferred, 0) < {declined}
       AND (r.last_shown IS NULL
            OR ?1 - r.last_shown >= {ladder})"
    )
}

fn utc() -> capture_time::Zone {
    capture_time::Zone::new("UTC").expect("UTC is always present")
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
    pub as_: capture_core::body::Promoted,
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

/// A moment a capture named, and what to say when it arrives.
///
/// Derived rather than stored. An `alert` event per dated capture would make
/// the log's size a function of how many things have dates, and would turn
/// correcting a time into a second event that has to find and supersede the
/// first. Computed, a corrected time simply produces a different answer next
/// time this is asked — which is what §3.1.3's additive promotion already
/// guarantees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Due {
    /// The note whose moment this is.
    pub tgt: EventId,
    /// When it comes due, as an instant.
    pub at: i64,
    /// Which moment of that note, for a series. `None` for a one-off — an
    /// occurrence at 9am Tuesday and the same series at 9am Wednesday are
    /// different moments of one note, and delivery keyed by note alone would
    /// deliver one of them.
    pub occ: Option<i64>,
    /// What to say. The capture's own text (§3.1.3 — nothing is summarised,
    /// and nothing is added that the capture did not contain).
    pub text: String,
    /// Whether the capture named a time of day rather than only a day.
    ///
    /// §2.6 splits immediate delivery from batched and gives examples rather
    /// than a rule; this is the rule. `dentist thursday 2pm` named 2pm, so 2pm
    /// is when it matters. `call the dentist tomorrow` named a day, and
    /// delivering it at 00:00 would obey the letter of a due date while
    /// missing what was meant. It is the instant-versus-floating-date
    /// distinction the time model already draws (§3.2), asked a different
    /// question.
    pub timed: bool,
}

/// One line of a list: an expanded item, or a task carrying its tag.
///
/// The two are deliberately the same shape. To whoever is reading a list they
/// are the same thing — something on it, done or not — and only the layer
/// underneath knows that one was written inside a list capture and the other
/// was tagged into place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListEntry {
    /// Where this line came from: the list capture, or the task's own note.
    pub tgt: EventId,
    /// Position as written, for an expanded item. Absent for a loose task,
    /// which has no place in anybody's written order.
    pub ord: Option<u32>,
    pub text: String,
    pub done: bool,
    /// The note carrying this line's task, once it has one. An item gains one
    /// by being ticked; before that there is nothing in the log about it.
    pub task: Option<EventId>,
}

/// Where an open task sits when a surface asks what is outstanding.
///
/// Decided here rather than by each surface. "Overdue" is a comparison against
/// today in a named zone, and a phone and a window working that out separately
/// is two answers on the same day — the same argument that put the descriptor
/// in the projection rather than in each renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    /// Due on a day that has passed.
    Overdue,
    /// Due on today or a day still to come.
    Due,
    /// Open, with no due date at all.
    ///
    /// The ordinary case rather than the exception. A bare task verb is below
    /// the confidence floor, so most tasks arrive by somebody saying one is a
    /// task — and somebody saying "send sarah the migration doc" is not saying
    /// when.
    Undated,
}

/// One open task, with the band it belongs to.
///
/// Finished tasks are absent entirely. §0 rates nothing that scores the user,
/// so there is no completed band to keep them for, and a surface asking what is
/// outstanding is not asking what was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRow {
    pub tgt: EventId,
    /// The one-line rendering the projection already keeps, so this surface
    /// cannot format the fields differently from any other.
    pub descriptor: String,
    /// The floating due date as stored — never an instant. "due Friday" is not
    /// 00:00 UTC on Friday (§3.6).
    pub due: Option<String>,
    pub band: Band,
}

/// An entity, as a surface reads it.
///
/// Assembled here rather than by each surface. A page is a display name, a
/// backing note and a count, and three surfaces computing those separately is
/// three answers to one question — the same argument that put the descriptor in
/// the projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityRow {
    pub ent: EntId,
    /// person | project | tag.
    pub kind: String,
    /// What to call it: the name somebody chose, or the earliest spelling.
    pub display: String,
    /// The note that *is* this entity, where one is bound.
    pub backing: Option<EventId>,
    /// Every spelling that reaches it, folded. More than one after a merge.
    pub names: Vec<String>,
    /// How many notes name it. Filled by the index; zero from a single read,
    /// which is cheaper and is what a page showing the notes does not need.
    pub count: u64,
}

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

    /// Whether a mute is still in force.
    ///
    /// One decision in one place, because asking it is not as simple as it
    /// looks and four surfaces had been getting it wrong by asking whether a
    /// mute *existed*: zero means indefinite rather than "ended in 1970", and
    /// any other value is an end that may already have passed. §2.6 makes a
    /// mute hold "permanently or until a stated date", so both readings have to
    /// be here or a lapsing mute never lapses.
    ///
    /// The queries in this file ask it in SQL and cannot call this, which is
    /// why ten of them spell it out. Keeping those in step is what
    /// `tools/duplicated.py` is for; this is at least the one copy every Rust
    /// caller shares.
    pub fn is_muted(&self, now: u64) -> bool {
        muted_in_force(self.muted_until, now)
    }
}

/// Whether a recorded mute holds at `now`: zero is indefinite, rather than
/// "ended in 1970", and any other value is an end that may have passed.
///
/// The one copy. The queries ask it through the SQL function `muted`, which
/// every connection registers (`db::apply`) — before that they spelled the
/// rule out ten times, three of them once wrongly, and `NoteRow::is_muted` was
/// an eleventh. A rule written in two languages is two rules.
pub fn muted_in_force(muted_until: Option<u64>, now: u64) -> bool {
    match muted_until {
        None => false,
        Some(0) => true,
        Some(ends) => ends > now,
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

    /// Whether this projection reflects the log, per origin.
    ///
    /// # Why the schema version was never enough
    ///
    /// `is_stale` asks whether the projection has the *shape* this build
    /// expects. It says nothing about whether it holds what the log holds, and
    /// those come apart in ways that are ordinary rather than exotic:
    ///
    /// - A sync round ingests forty events and the connection drops before
    ///   the fold. The events are committed, the log's heads have moved, and
    ///   anti-entropy will never re-offer them because they are no longer
    ///   wanted. They exist and no surface shows them.
    /// - The projection is deleted — which §8.8 says is safe at any time, and
    ///   it is, provided something rebuilds. Opening creates a fresh empty one
    ///   and stamps the current schema into it, so `is_stale` answers *false*
    ///   and the whole archive is silently invisible.
    /// - A process is killed between `log.append` and the incremental fold.
    ///
    /// Each of those was reachable and none of them said anything. The
    /// `fold_head` table has existed since M1 for exactly this comparison and
    /// was written by `rebuild` and read by nothing.
    ///
    /// Compares per origin rather than by a count: events do not arrive in one
    /// global order, and a single number either skips what landed late or
    /// reports a mismatch every time something does.
    pub fn behind(&self, log_heads: &std::collections::BTreeMap<String, u64>) -> Result<bool> {
        for (dev, seq) in log_heads {
            if self.fold_head(dev)? < *seq {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn set_meta(&self, k: &str, v: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO meta (k, v) VALUES (?1, ?2) ON CONFLICT(k) DO UPDATE SET v = ?2",
            params![k, v],
        )?;
        Ok(())
    }

    /// Replace everything with a freshly folded projection.
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
             DELETE FROM occurrences; DELETE FROM review; DELETE FROM intents;
             DELETE FROM entities; DELETE FROM entity_names;",
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
    /// listed by `capture tasks`, and was invisible on the day it mattered. That
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

    /// Every open task, banded into overdue, due and undated.
    ///
    /// One query rather than three. `tasks_overdue` and `tasks_due_on` both
    /// require `due IS NOT NULL`, so between them they cannot reach a task with
    /// no due date — which is most of them, and the reason this exists. Asking
    /// three times would also give three chances to disagree about what today
    /// is, since each call would carry its own.
    ///
    /// `today` is a floating date in the reading zone, compared as a string.
    /// Both sides are ISO-8601 days, where lexical and chronological order are
    /// the same thing, and neither is an instant — comparing against one would
    /// make a due date an appointment nobody made.
    ///
    /// Dated tasks come first in due order, undated ones after in capture
    /// order, newest first.
    pub fn tasks_outstanding(&self, today: &str) -> Result<Vec<TaskRow>> {
        let mut st = self.conn.prepare(
            "SELECT p.tgt, p.descriptor, p.due
             FROM promotions p JOIN notes n ON n.tgt = p.tgt
             WHERE p.kind = 'task' AND COALESCE(p.done, 0) = 0
             ORDER BY (p.due IS NULL) ASC, p.due ASC, n.created_wall DESC",
        )?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (id, descriptor, due) = r?;
            let Some(tgt) = id_from(id) else { continue };
            let band = match due.as_deref() {
                None => Band::Undated,
                Some(d) if d < today => Band::Overdue,
                Some(_) => Band::Due,
            };
            out.push(TaskRow {
                tgt,
                descriptor,
                due,
                band,
            });
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
        let eligible = review_eligible();
        let sql = format!(
            "SELECT {NOTE_COLUMNS}
             FROM notes n
             JOIN promotions p ON p.tgt = n.tgt
             LEFT JOIN review r ON r.tgt = n.tgt
             WHERE {eligible}
               AND ((p.start_ms IS NOT NULL AND p.start_ms > ?1)
                 OR (p.start_date IS NOT NULL AND p.start_date > ?3))
             ORDER BY n.created_wall ASC
             LIMIT ?2"
        );
        // The only tier that compares a date, so the only one given one.
        let today = format!("{}", capture_time::civil_date(now, utc()));
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
        let eligible = review_eligible();
        let sql = format!(
            "SELECT {NOTE_COLUMNS}
             FROM notes n
             LEFT JOIN review r ON r.tgt = n.tgt
             WHERE {eligible}
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
        let eligible = review_eligible();
        let sql = format!(
            "SELECT {NOTE_COLUMNS}
             FROM notes n
             LEFT JOIN review r ON r.tgt = n.tgt
             WHERE {eligible}
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
                    capture_core::body::Promoted::Series(s) => s.carries_forward,
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
                    capture_core::body::Promoted::Series(s) => s.carries_forward,
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
             {UNREAD_FROM_WHERE}
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

    /// Record that the sweep read this note, and what it read it at.
    ///
    /// Called for every note the sweep looks at, **including the ones it
    /// promotes to nothing** — that is the whole purpose. Writing only on a
    /// promotion is what left ordinary captures queued for ever; see `read_by`
    /// in the schema for what that cost and why this is not in the log.
    /// Notes asking which of two days they mean, where one of the two is
    /// `date`. Not where a person pinned a promotion: they already answered.
    pub fn unclear_days_on(&self, date: &str) -> Result<Vec<(EventId, [String; 2])>> {
        let mut st = self.conn.prepare(
            "SELECT u.tgt, u.first, u.second FROM unclear_days u
               LEFT JOIN promotions p ON p.tgt = u.tgt
              WHERE (u.first = ?1 OR u.second = ?1)
                AND (p.tgt IS NULL OR p.pinned = 0)
              ORDER BY u.tgt",
        )?;
        let rows = st.query_map(params![date], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                [r.get::<_, String>(1)?, r.get::<_, String>(2)?],
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (t, days) = row?;
            if let Some(id) = id_from(t) {
                out.push((id, days));
            }
        }
        Ok(out)
    }

    /// The two days one note asks between, if it asks. The same condition as
    /// [`Store::unclear_days_on`].
    pub fn unclear_day_of(&self, tgt: &EventId) -> Result<Option<[String; 2]>> {
        Ok(self
            .conn
            .query_row(
                "SELECT u.first, u.second FROM unclear_days u
                   LEFT JOIN promotions p ON p.tgt = u.tgt
                  WHERE u.tgt = ?1 AND (p.tgt IS NULL OR p.pinned = 0)",
                params![tgt.0.to_vec()],
                |r| Ok([r.get::<_, String>(0)?, r.get::<_, String>(1)?]),
            )
            .optional()?)
    }

    pub fn mark_read(&self, tgt: &EventId, grammar: u32, at: (u64, u32)) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO read_by (tgt, grammar, wall, counter)
             VALUES (?1,?2,?3,?4)",
            params![tgt.0.to_vec(), grammar as i64, at.0 as i64, at.1 as i64],
        )?;
        Ok(())
    }

    /// How many captures have not been read.
    ///
    /// The same question [`Store::needing_parse`] answers, counted instead of
    /// listed, and **deliberately available on a read-only handle**. That is
    /// the whole point of it: a surface has to be able to say "there are three
    /// captures nobody has read yet" without becoming a writer, because §8.8
    /// puts reads on a path that does not involve the writer at all — and
    /// because the moment this most needs saying is the moment the writer has
    /// not been running.
    ///
    /// Shares `UNREAD_FROM_WHERE` with the sweep's own queue rather than
    /// restating it. Two copies of this predicate would drift, and the drift
    /// would read as "nothing outstanding" over a backlog, or as a line that
    /// never goes away however often the sweep runs.
    pub fn unread_count(&self, grammar: u32) -> Result<usize> {
        let sql = format!("SELECT COUNT(*) {UNREAD_FROM_WHERE}");
        Ok(self
            .conn
            .query_row(&sql, params![grammar as i64], |r| r.get::<_, i64>(0))? as usize)
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
               AND NOT muted(n.muted_until, ?2)
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

    /// The names among these that the projection knows.
    ///
    /// Feeds an incremental fold, which sees only the events of the notes that
    /// changed and would otherwise resolve every link to nothing.
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
                        capture_core::hlc::Hlc {
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

    /// Every note that wrote a given spelling.
    ///
    /// Literal, and deliberately not the entity page: this matches
    /// `mentions.value` as typed, so a merge does not change the answer. The
    /// page that follows merges is [`Store::entity_notes`], which joins through
    /// `entity_names` to reach every spelling of one entity. Both exist because
    /// they answer different questions — "which notes say this" and "which
    /// notes are about this person" — and §4.3 only promises the second.
    ///
    /// Either way there is no entity record to create and nothing to maintain:
    /// a page exists for every name the moment it is written, which is what
    /// makes this usable on day one rather than after months of curation.
    ///
    /// Muted notes are excluded, for the same reason they are excluded from
    /// search: §2.6 makes muting the cheap way to stop something resurfacing,
    /// and a page that kept showing it would defeat that.
    pub fn mentioning(&self, sigil: char, value: &str, now: u64) -> Result<Vec<NoteRow>> {
        let sql = format!(
            "SELECT {NOTE_COLUMNS}
             FROM notes n JOIN mentions m ON m.tgt = n.tgt
             WHERE m.sigil = ?1 AND m.value = ?2
               AND NOT muted(n.muted_until, ?3)
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

    /// Which entity a written spelling reaches, after every merge.
    ///
    /// Case-folded before the lookup, matching what the id was derived from, so
    /// `@Sarah` and `@sarah` ask the same question.
    pub fn entity_id(&self, kind: &str, name: &str) -> Result<Option<EntId>> {
        let got: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT ent_id FROM entity_names WHERE kind = ?1 AND name = ?2",
                params![kind, casefold(name)],
                |r| r.get(0),
            )
            .optional()?;
        Ok(got.and_then(|b| b.try_into().ok()).map(EntId))
    }

    /// An entity, by a spelling that reaches it.
    ///
    /// `None` means no note has ever named it — which is an ordinary answer, not
    /// a missing record. §3.7's whole claim is that this axis populates without
    /// anybody creating anything, so there is nothing here to have failed.
    pub fn entity(&self, kind: &str, name: &str) -> Result<Option<EntityRow>> {
        let Some(ent) = self.entity_id(kind, name)? else {
            return Ok(None);
        };
        self.entity_by_id(&ent)
    }

    /// An entity by its identity.
    pub fn entity_by_id(&self, ent: &EntId) -> Result<Option<EntityRow>> {
        let row = self
            .conn
            .query_row(
                "SELECT kind, display, note_tgt FROM entities WHERE ent_id = ?1",
                params![ent.0.to_vec()],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, Option<Vec<u8>>>(2)?,
                    ))
                },
            )
            .optional()?;
        let Some((kind, display, backing)) = row else {
            return Ok(None);
        };
        Ok(Some(EntityRow {
            ent: *ent,
            kind,
            display,
            backing: backing.and_then(id_from),
            names: self.spellings_of(ent)?,
            count: 0,
        }))
    }

    /// Every written spelling that reaches an entity, so a page can say what it
    /// has been called as well as what it is called.
    fn spellings_of(&self, ent: &EntId) -> Result<Vec<String>> {
        let mut st = self
            .conn
            .prepare("SELECT name FROM entity_names WHERE ent_id = ?1 ORDER BY name")?;
        let rows = st.query_map(params![ent.0.to_vec()], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// The notes naming an entity, newest first.
    ///
    /// The union §4.3 needs and `mentioning` could not do: every spelling that
    /// reaches this entity, not the one string somebody happened to type. A
    /// backing note is left out, because the page it belongs to already shows it
    /// above this list and a row appearing twice reads as two notes.
    ///
    /// Muted notes are excluded on the same terms as everywhere else, and a
    /// redacted note is absent because it has no text and so names nobody.
    pub fn entity_notes(&self, ent: &EntId, now: u64) -> Result<Vec<NoteRow>> {
        let backing = self
            .entity_by_id(ent)?
            .and_then(|e| e.backing)
            .map(|b| b.0.to_vec());
        // `mentions.value` keeps the spelling as written and `entity_names.name`
        // is folded, so the join folds the mention side to match. `lower` rather
        // than anything cleverer, because `EntId::derive` folds ASCII only and
        // the two have to agree exactly or a name reaches no entity.
        let sql = format!(
            "SELECT DISTINCT {NOTE_COLUMNS}
             FROM notes n
             JOIN mentions m ON m.tgt = n.tgt
             JOIN entity_names en
               ON en.kind = CASE m.sigil
                              WHEN '@' THEN 'person'
                              WHEN '^' THEN 'project'
                              WHEN '#' THEN 'tag'
                            END
              AND en.name = lower(m.value)
             WHERE en.ent_id = ?1
               AND NOT muted(n.muted_until, ?2)
             ORDER BY n.created_wall DESC"
        );
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params![ent.0.to_vec(), now as i64], note_row)?;
        let mut out = Vec::new();
        for r in rows {
            let row: NoteRow = r?;
            if backing.as_deref() == Some(row.tgt.0.as_slice()) {
                continue;
            }
            out.push(row);
        }
        Ok(out)
    }

    /// Every entity of a kind, with how many notes name it.
    ///
    /// Merged-away entities are absent: only the survivor of a chain has a row,
    /// so a merge takes a name out of this index without anything having to
    /// remove it.
    pub fn entities_of_kind(&self, kind: &str, now: u64) -> Result<Vec<EntityRow>> {
        // Three grouped queries rather than three per entity.
        //
        // The first version asked `entity_notes` once per row, and each of those
        // brought its own `entity_by_id` and `spellings_of` with it. Measured at
        // 301 people it took **486ms** — worse than the 66ms per-row read that
        // `read_as` was rewritten for, and on a surface somebody opens to
        // browse. The shape was the bug rather than the volume, which is why the
        // measurement is kept as a test.
        let mut st = self.conn.prepare(
            "SELECT ent_id, display, note_tgt FROM entities
              WHERE kind = ?1 ORDER BY lower(display)",
        )?;
        let rows = st.query_map(params![kind], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<Vec<u8>>>(2)?,
            ))
        })?;
        let mut out: Vec<EntityRow> = Vec::new();
        for row in rows {
            let (id, display, backing) = row?;
            let Some(ent) = ent_from(id) else { continue };
            out.push(EntityRow {
                ent,
                kind: kind.to_string(),
                display,
                backing: backing.and_then(id_from),
                names: Vec::new(),
                count: 0,
            });
        }
        drop(st);

        // Every spelling, in one pass.
        let mut st = self
            .conn
            .prepare("SELECT ent_id, name FROM entity_names WHERE kind = ?1 ORDER BY name")?;
        let spellings = st.query_map(params![kind], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut by_ent: BTreeMap<EntId, Vec<String>> = BTreeMap::new();
        for sp in spellings {
            let (id, name) = sp?;
            if let Some(ent) = ent_from(id) {
                by_ent.entry(ent).or_default().push(name);
            }
        }
        drop(st);

        // And every count, grouped. The backing note is excluded here as it is
        // in `entity_notes`, so the number matches the list somebody then sees.
        let mut st = self.conn.prepare(
            "SELECT en.ent_id, COUNT(DISTINCT n.tgt)
               FROM notes n
               JOIN mentions m ON m.tgt = n.tgt
               JOIN entity_names en
                 ON en.kind = CASE m.sigil
                                WHEN '@' THEN 'person'
                                WHEN '^' THEN 'project'
                                WHEN '#' THEN 'tag'
                              END
                AND en.name = lower(m.value)
               JOIN entities e ON e.ent_id = en.ent_id
              WHERE en.kind = ?1
                AND (e.note_tgt IS NULL OR n.tgt != e.note_tgt)
                AND NOT muted(n.muted_until, ?2)
              GROUP BY en.ent_id",
        )?;
        let counts = st.query_map(params![kind, now as i64], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?))
        })?;
        let mut count_of: BTreeMap<EntId, u64> = BTreeMap::new();
        for c in counts {
            let (id, n) = c?;
            if let Some(ent) = ent_from(id) {
                count_of.insert(ent, n.max(0) as u64);
            }
        }
        drop(st);

        for row in &mut out {
            row.names = by_ent.remove(&row.ent).unwrap_or_default();
            row.count = count_of.get(&row.ent).copied().unwrap_or(0);
        }
        Ok(out)
    }

    /// Every moment this archive names that has come due, oldest first.
    ///
    /// Asking does not deliver. Three surfaces and the tests all ask this, and
    /// one that fired as a side effect could not be asked twice — nor could
    /// `capture due` exist, which is the answer to "why did nothing happen".
    ///
    /// # What is deliberately not here
    ///
    /// Muted notes, completed tasks and occurrences, cancelled occurrences,
    /// and notes whose content is withheld or destroyed. Each is already a
    /// fact in the projection, so none of them needs an alert cancelled —
    /// they simply stop being returned. A task with no due date is absent for
    /// the same reason it is never overdue: §3.6 refuses the manufactured
    /// backlog, and a thing with no date has no moment to arrive.
    /// `horizon` is how far ahead to look; `now` is when it actually is.
    ///
    /// Two parameters rather than one, because they answer different
    /// questions and one value answered both wrongly. Asking thirty days
    /// ahead — which is what arming the next alarm does — evaluated every
    /// mute as if thirty days had already passed, so a note somebody muted
    /// for a week came back as due. §2.6 makes muting the cheap way to stop
    /// something resurfacing, and a horizon that expires mutes early is that
    /// gesture quietly not working.
    pub fn due_by(&self, horizon: i64, now: i64, zone: &str) -> Result<Vec<Due>> {
        let mut out = Vec::new();

        // Events, which named an instant.
        //
        // # Why the `done` column is not the whole test
        //
        // `promotions.done` is only ever non-null for a task. For an event it
        // is always NULL, so `COALESCE(p.done, 0) = 0` is a filter that is
        // always true — and completing a one-off event is not recorded there at
        // all. A tick writes an *occurrence exception* against the promotion's
        // own start, which is the path the agenda uses, and that suppresses the
        // occurrence row while leaving this one due.
        //
        // So the workshop somebody ticked off this morning announced itself
        // this afternoon, and cancelling made no difference either — `OCC_CANCEL`
        // has the identical shape.
        //
        // Moving one was worse, because it produced *two* rows: this one at the
        // instant the event no longer starts, and an occurrence row at the
        // instant it now does. Two alert keys, two notifications, one event,
        // and the first of them for a time that had been deliberately changed.
        //
        // An occurrence row against this promotion's own start is therefore the
        // authority on that moment, whatever it says: done, cancelled, moved or
        // merely present. The occurrence query below reports it if it is still
        // due, and reports nothing if it is not.
        let mut st = self.conn.prepare(&format!(
            "SELECT p.tgt, p.start_ms, n.text
               FROM promotions p JOIN notes n ON n.tgt = p.tgt
              WHERE p.kind = 'event' AND p.start_ms IS NOT NULL AND p.start_ms <= ?1
                AND COALESCE(p.done, 0) = 0
                AND NOT EXISTS (
                  SELECT 1 FROM occurrences o
                   WHERE o.tgt = p.tgt AND o.occ = p.start_ms
                )
                AND {ALERTABLE}",
        ))?;
        for r in st.query_map(params![horizon, now], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })? {
            let (tgt, at, text) = r?;
            if let Some(tgt) = id_from(tgt) {
                out.push(Due {
                    tgt,
                    at,
                    occ: None,
                    text,
                    timed: true,
                });
            }
        }

        // Occurrences of a series. Each is its own moment; a cancelled or
        // completed one is not.
        //
        // An occurrence with no time of day is a day, delivered the way a
        // task's due date is rather than as a timed alarm at midnight. And one
        // that has no time only because its hour said neither am nor pm says
        // nothing until somebody answers: any moment picked to announce it
        // would be the guess the recognizer refused to make. Found on a phone
        // where "gym every monday at 2", captured on a Monday afternoon,
        // announced "was due earlier" at once.
        let mut st = self.conn.prepare(&format!(
            "SELECT o.tgt, o.occ, o.start_ms, n.text, o.all_day, COALESCE(p.pinned, 0)
               FROM occurrences o JOIN notes n ON n.tgt = o.tgt
               LEFT JOIN promotions p ON p.tgt = o.tgt
              WHERE o.start_ms <= ?1
                AND o.cancelled = 0 AND o.done = 0
                AND {ALERTABLE}",
        ))?;
        for r in st.query_map(params![horizon, now], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)? != 0,
                r.get::<_, i64>(5)? != 0,
            ))
        })? {
            let (tgt, occ, at, text, all_day, pinned) = r?;
            if !pinned && capture_parse::unclear_hour(&text).is_some() {
                continue;
            }
            if let Some(tgt) = id_from(tgt) {
                out.push(Due {
                    tgt,
                    at,
                    occ: Some(occ),
                    text,
                    timed: !all_day,
                });
            }
        }

        // Tasks, which named a day rather than an instant. The moment is the
        // start of that day in the reader's zone — a floating date has no
        // instant of its own (§3.2), and this is where one is chosen. Not
        // time-critical: delivering at 00:00 would be obeying the letter of a
        // due date and missing what somebody meant by "tomorrow".
        let mut st = self.conn.prepare(&format!(
            "SELECT p.tgt, p.due, n.text
               FROM promotions p JOIN notes n ON n.tgt = p.tgt
              WHERE p.kind = 'task' AND p.due IS NOT NULL
                AND COALESCE(p.done, 0) = 0
                AND {ALERTABLE}",
        ))?;
        for r in st.query_map(params![horizon, now], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })? {
            let (tgt, day, text) = r?;
            let Some(tgt) = id_from(tgt) else { continue };
            let Some(date) = capture_time::FloatingDate::parse(&day) else {
                continue;
            };
            let Some(zone) = capture_time::Zone::new(zone) else {
                continue;
            };
            let Some(at) = capture_time::at_local(date, 0, 0, zone) else {
                continue;
            };
            if at <= horizon {
                out.push(Due {
                    tgt,
                    at,
                    occ: None,
                    text,
                    timed: false,
                });
            }
        }

        // Oldest first: a backlog delivered in the order it accumulated reads
        // as a day that happened, rather than as a pile.
        out.sort_by_key(|d| (d.at, d.tgt.0, d.occ));
        Ok(out)
    }

    /// The newest version of a note — what a further edit should follow.
    ///
    /// The `notes` table has tracked this since M1 and nothing read it, which
    /// is how every edit came to name the note's *root* as its predecessor
    /// rather than the version it was editing. `resolve` computes tips as the
    /// versions nobody names as `prev`, so with the root named every time,
    /// every edit after the first was a tip — and two sequential edits on one
    /// device, a minute apart, reported as a divergence.
    ///
    /// The conflict signal is only worth having if it is quiet when nothing
    /// conflicts.
    pub fn head_of(&self, tgt: &EventId) -> Result<Option<EventId>> {
        Ok(self
            .conn
            .query_row(
                "SELECT head FROM notes WHERE tgt = ?1",
                params![tgt.0.to_vec()],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .optional()?
            .and_then(id_from))
    }

    /// A list: the items expanded under a tag, then the tasks carrying it.
    ///
    /// §3.7 makes a collection a saved query rather than a folder, and this is
    /// that query. A list is not a container anything was put into — it is
    /// everything that answers to a tag, which is why a task tagged by hand
    /// joins the list it belongs to without being copied into it (§3.6).
    ///
    /// # Two populations, one sequence
    ///
    /// Items come from `list_items`, expanded from a list capture's text and
    /// ordered as they were written. Tasks come from the tag itself. A task
    /// whose text matches an item **is** that item and is returned once —
    /// otherwise every ticked item would appear twice, once as the item and
    /// once as the task that ticked it.
    ///
    /// Items first in written order, then loose tasks newest first. Items have
    /// an order somebody chose and tasks do not, and interleaving the two by
    /// time would scramble the one thing that was deliberate.
    pub fn list(&self, tag: &str, now: u64) -> Result<Vec<ListEntry>> {
        // A collection is gathered by a tag, and an untagged checklist has
        // none. Its items belong to the capture that wrote them and are read
        // through `items_of`; gathering every untagged list in the archive
        // under one empty name would be the opposite of what writing no tag
        // meant.
        if tag.is_empty() {
            return Ok(Vec::new());
        }

        // Muted list captures are excluded here for the same reason muted
        // notes are excluded from a tag page and from search (§2.6): muting is
        // the cheap way to stop something resurfacing, and a surface that kept
        // showing it would defeat that.
        let mut st = self.conn.prepare(
            "SELECT li.tgt, li.ord, li.item, li.done
               FROM list_items li JOIN notes n ON n.tgt = li.tgt
              WHERE li.tag = ?1
                AND NOT muted(n.muted_until, ?2)
              ORDER BY n.created_wall ASC, li.ord ASC",
        )?;
        let rows = st.query_map(params![tag, now as i64], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;

        let mut items = Vec::new();
        for r in rows {
            let (tgt, ord, text, done) = r?;
            let Some(tgt) = id_from(tgt) else { continue };
            items.push(ListEntry {
                tgt,
                ord: Some(ord as u32),
                text,
                // Read back from the fold, which read it from the log. An item
                // is never a task, so there is nothing to match against.
                done: done != 0,
                task: None,
            });
        }

        // Which captures' items are already on this list. A task that *is* one
        // of them is not a separate thing beside its own contents.
        //
        // Reachable the moment a list capture is promoted to a task, which is
        // an ordinary thing to do to a shopping list: the capture then appeared
        // under its own items and was counted with them, so a three-item list
        // reported four things outstanding. The comment below reasons carefully
        // about a task that is genuinely separate, and did not anticipate the
        // one that owns the items.
        let owners: Vec<EventId> = items.iter().map(|e| e.tgt).collect();

        // The tag's own notes, which carry whatever completion there is.
        let mut loose = Vec::new();
        for note in self.tagged(tag, now)? {
            if owners.contains(&note.tgt) {
                continue;
            }
            let Some(text) = note.text.as_deref() else {
                // Redacted, or queued for it. A list must not become a way to
                // read back what redaction removed.
                continue;
            };
            let done = self.task_done(&note.tgt)?;

            // Only a task joins the list. A tag gathers every kind of note —
            // `#standup at 9:30` carries one and is not something anybody
            // ticks off — and drawing those with an empty box claims they are
            // actionable when nothing in the log says so. The tag page
            // (`capture mentions`) holds everything carrying a tag; a list is
            // the actionable half.
            //
            // No longer matched against the expanded items by text. That match
            // existed because ticking used to mint a task per item, and an
            // item and its own minted task would otherwise have appeared as
            // two rows. Nothing is minted now, so a task carrying this tag is
            // a task somebody captured — a separate thing that belongs on the
            // list beside the items, not a duplicate of one.
            if done.is_some() {
                loose.push(ListEntry {
                    tgt: note.tgt,
                    ord: None,
                    text: text.to_string(),
                    done: done.unwrap_or(false),
                    task: Some(note.tgt),
                });
            }
        }

        items.extend(loose);
        Ok(items)
    }

    /// Whether a note is a task, and whether it is done.
    ///
    /// `None` where the note is not a task at all, which is a different answer
    /// from an open one: an item that has never been ticked has nothing in the
    /// log about it, and saying "open" would claim a promotion nobody wrote.
    pub fn task_done(&self, tgt: &EventId) -> Result<Option<bool>> {
        Ok(self
            .conn
            .query_row(
                "SELECT COALESCE(done, 0) FROM promotions WHERE tgt = ?1 AND kind = 'task'",
                params![tgt.0.to_vec()],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
            .map(|d| d != 0))
    }

    /// The tag a note opens a list under, where it opens one.
    ///
    /// Read back out of the expansion rather than re-parsed from the text. The
    /// fold already answered this question, and a second answer computed on
    /// the way to a surface is one that can disagree with the rows the same
    /// surface is about to draw.
    pub fn list_tag_of(&self, tgt: &EventId) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT tag FROM list_items WHERE tgt = ?1 ORDER BY ord LIMIT 1",
                params![tgt.0.to_vec()],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
    }

    /// The items one capture names, with their completion.
    ///
    /// Narrower than [`Store::list`] on purpose. A row drawing itself wants
    /// its own items; `list` answers the tag-wide question, which means
    /// gathering every capture under the tag *and* every task carrying it.
    /// Called once per row that is a list, that is a full tag query per row —
    /// measured at 66ms for a hundred rows, which is a scroll somebody can
    /// feel.
    pub fn items_of(&self, tgt: &EventId) -> Result<Vec<ListEntry>> {
        let mut st = self
            .conn
            .prepare("SELECT ord, item, done FROM list_items WHERE tgt = ?1 ORDER BY ord ASC")?;
        let rows = st.query_map(params![tgt.0.to_vec()], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (ord, text, done) = r?;
            out.push(ListEntry {
                tgt: *tgt,
                ord: Some(ord as u32),
                text,
                done: done != 0,
                task: None,
            });
        }
        Ok(out)
    }

    /// How many items on a list carry this exact text.
    ///
    /// Two items reading the same thing are indistinguishable to completion,
    /// which matches by text — so ticking one has to report the ambiguity
    /// rather than pick. Counting is separate from reading because the caller
    /// that needs this is refusing to act, not drawing anything.
    pub fn list_item_count(&self, tag: &str, item: &str) -> Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM list_items WHERE tag = ?1 AND item = ?2",
            params![tag, item],
            |r| r.get(0),
        )?;
        Ok(n as usize)
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
               AND NOT muted(n.muted_until, ?2)
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
    /// Takes the clock because a mute can end. Both branches asked only whether
    /// an end existed, so a tag whose notes were muted until Monday stayed out
    /// of the count for good — the same defect as the review predicate, in the
    /// surface that feeds the window's tag sidebar.
    ///
    /// Ordered by how often each is mentioned, then alphabetically. Frequency
    /// first because a sidebar is read top-down and the name you want is
    /// usually one you use — and because alphabetical alone puts whoever is
    /// called Aaron permanently above everyone you actually work with.
    pub fn distinct_mentions(&self, sigil: char, now: u64) -> Result<Vec<(String, u32)>> {
        // Tags come from two places, so `#` unions them. The other two sigils
        // exist only in the text and the union is a no-op for them, but writing
        // it once keeps the three consistent rather than special-casing.
        let mut st = self.conn.prepare(
            "SELECT value, COUNT(DISTINCT tgt) AS n FROM (
               SELECT m.value AS value, m.tgt AS tgt
                 FROM mentions m JOIN notes n ON n.tgt = m.tgt
                 WHERE m.sigil = ?1 AND n.redacted_at IS NULL
                   AND NOT muted(n.muted_until, ?2)
               UNION
               SELECT t.tag AS value, t.tgt AS tgt
                 FROM tags t JOIN notes n ON n.tgt = t.tgt
                 WHERE ?1 = '#' AND n.redacted_at IS NULL
                   AND NOT muted(n.muted_until, ?2)
             )
             GROUP BY value
             ORDER BY n DESC, value ASC",
        )?;
        let rows = st.query_map(params![sigil.to_string(), now as i64], |r| {
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
            // One offer per note: its chosen name where that matches, its slug
            // otherwise. Both were offered, so a named page appeared twice —
            // `packing-list` and `packing-list-p1ge8kq6f2` — and the second is
            // the name nobody wants to write (pages, 2026-10-13).
            "SELECT CASE WHEN alias LIKE ?1 ESCAPE '\\' THEN alias ELSE slug END
               FROM notes
              WHERE (slug LIKE ?1 ESCAPE '\\' OR alias LIKE ?1 ESCAPE '\\')
                AND redacted_at IS NULL
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
            params![t, capture_core::slug::derive_redacted(tgt).text, at as i64],
        )?;
        tx.execute("DELETE FROM links WHERE src = ?1", params![t])?;

        // Everything else derived from the text, and this was missing.
        //
        // `write_into` refuses to extract mentions from a note with no readable
        // text, and the comment above it says why: an entity page must not
        // become a way to read what redaction removed. But that guard only runs
        // on a fold, and a purge is not one — it edits the projection in place.
        // So a destroyed note went on naming every person, project and tag it
        // had mentioned, on every one of their pages, for ever. No text was
        // readable, which is why nothing looked wrong; what survived was the
        // fact of the mention, which is the part §1.1 is about.
        //
        // `list_items` is the same story. An expanded list kept its items after
        // the capture they came from was destroyed.
        tx.execute("DELETE FROM mentions WHERE tgt = ?1", params![t])?;
        tx.execute("DELETE FROM list_items WHERE tgt = ?1", params![t])?;
        // Two days the text named. Not words, but derived from them.
        tx.execute("DELETE FROM unclear_days WHERE tgt = ?1", params![t])?;
        // The expanded schedule, and what was written about single occurrences
        // of it. `title`, `location` and `notes` come from `OCC_EDIT` bodies the
        // purge has just destroyed, so leaving the rows is the same failure as
        // leaving the mentions — the body gone and a derived copy of the same
        // material still readable. The dates and the completions are not text,
        // but they are a schedule belonging to a capture that no longer exists.
        //
        // The seventh table found missing from this function, and the first the
        // guard caught rather than a person: its fixture had been a one-off
        // capture, so `occurrences` was empty and counted clean.
        tx.execute("DELETE FROM occurrences WHERE tgt = ?1", params![t])?;
        // The tags put on it, and the events that put them there.
        //
        // The eighth and ninth, and the argument is the one directly above
        // about mentions: what survives is the *fact*, and the fact is the part
        // §1.1 is about. `#medical` on a destroyed note still says what the note
        // was about, and a tag listing would draw the tombstone under it.
        //
        // What settles it is the inconsistency otherwise. `#vault` typed into
        // the text is a mention and goes; `#vault` applied with a tag event
        // would stay. One word, two tables, opposite outcomes depending on how
        // it was attached — and the comment above already says a destroyed note
        // must not go on naming "every person, project and tag it had
        // mentioned".
        tx.execute("DELETE FROM tags WHERE tgt = ?1", params![t])?;
        tx.execute("DELETE FROM tag_adds WHERE tgt = ?1", params![t])?;
        // The context it was captured in: the application, the window title,
        // the working directory, the repository and the branch.
        //
        // The third table to be found surviving a purge, after the two above,
        // and for the same reason — a purge edits the projection in place, so
        // every derived table has to be named here or it is simply missed.
        //
        // §1.1's argument about the search index applies unchanged: destroying
        // the body while leaving a derived copy of the same material is the
        // stated purpose failing. A window title is frequently the same
        // material as the note beside it — "Reset password — Fastmail" next to
        // a destroyed note about a password — and `by_context` joined this
        // table with no redaction filter, so a destroyed note went on being
        // returned by a context query with its context attached.
        tx.execute("DELETE FROM ambient WHERE tgt = ?1", params![t])?;
        // The queued destruction, now that it has happened.
        //
        // The fold already declines to queue a note whose body is gone — it
        // stops setting `redaction_pending` — so a rebuilt projection has no
        // row. A purge edits in place, and nothing deleted what the fold would
        // no longer write, so the row stayed for ever and every later drain
        // destroyed the note again: an intent recorded, a snapshot attempted,
        // rows already gone deleted, an UPDATE that changed nothing, and a
        // write-ahead-log checkpoint — paid on every drain, multiplied by every
        // redaction the archive had ever carried out.
        //
        // It also let `cancel_redaction` find a row for a destruction that had
        // already happened, write the cancellation, and report that nothing was
        // destroyed over content that was gone.
        //
        // The fourth derived table found surviving a purge, after mentions,
        // list items and ambient.
        tx.execute("DELETE FROM redact_queue WHERE tgt = ?1", params![t])?;
        // What the recognizer made of it.
        //
        // `promotions` holds the rendered descriptor and the fields as CBOR,
        // and for an event those fields carry the location and the attendees —
        // all of it derived from the text being destroyed. §1.1's argument
        // about the search index is the same argument: a derived copy of the
        // material is the material.
        //
        // It also stops a destroyed note appearing as a task or an appointment,
        // which it should not, having no content left to be one about.
        //
        // Found by the guard in `tests/purge_leaves_nothing.rs` on its first
        // run, which is what that test is for — the fifth and sixth tables
        // caught surviving a purge, and the first two not found by hand.
        tx.execute("DELETE FROM promotions WHERE tgt = ?1", params![t])?;
        tx.execute("DELETE FROM promotion_at WHERE tgt = ?1", params![t])?;
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

    /// Preferences that were actually chosen, with who chose them and when.
    ///
    /// A key that is absent has never been chosen. That absence is the whole
    /// point: it is what lets status say "default" rather than reporting a
    /// value with no indication of where it came from.
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

    /// Notes whose cancellation arrived after they had been destroyed.
    ///
    /// Somebody who cancels a redaction believes the content survived. On the
    /// device that cancelled it may have; on this one it did not, and §1.1 asks
    /// for that to be reported rather than discarded.
    pub fn late_cancels(&self) -> Result<Vec<EventId>> {
        let mut st = self.conn.prepare("SELECT tgt FROM late_cancels")?;
        let rows = st.query_map([], |r| r.get::<_, Vec<u8>>(0))?;
        let mut out = Vec::new();
        for r in rows {
            if let Ok(id) = <[u8; 32]>::try_from(r?.as_slice()) {
                out.push(EventId(id));
            }
        }
        Ok(out)
    }

    /// Which events are held, and why.
    ///
    /// `held_count` was the only accessor anywhere, so a surface could say
    /// "1 event(s) stored but not folded" and nothing could say *which*, of
    /// what kind, or why. A condition with nowhere to go is one somebody reads
    /// once and then learns to skip — and in this case the count was wrong, and
    /// unlistable meant unarguable.
    ///
    /// The kind comes from the caller's log rather than from here: the
    /// projection stores the reason, and what the event actually *was* is in
    /// the events table where it has always been.
    pub fn held(&self) -> Result<Vec<(EventId, String)>> {
        let mut st = self
            .conn
            .prepare("SELECT event_id, reason FROM held ORDER BY event_id")?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, String>(1)?))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (id, reason) = r?;
            if let Ok(id) = <[u8; 32]>::try_from(id.as_slice()) {
                out.push((EventId(id), reason));
            }
        }
        Ok(out)
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
    p: &capture_core::fold::Promotion,
) -> Result<()> {
    use capture_core::body::{Promoted, When};

    let (kind, start_ms, end_ms, start_date, end_date, due, done) = match &p.as_ {
        Promoted::Note => ("note", None, None, None, None, None, None),
        Promoted::Page => ("page", None, None, None, None, None, None),
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
    note: &capture_core::fold::Note,
    promotion: &capture_core::fold::Promotion,
    all: &std::collections::BTreeMap<EventId, capture_core::fold::Note>,
    now: i64,
) -> Result<()> {
    use capture_core::body::{Promoted, When};

    let mut written: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();

    if let Promoted::Series(sf) = &promotion.as_ {
        let (from, to) = capture_time::horizon(now);

        // An all-day series is expanded at local midnight purely so the rows
        // order; `all_day` is what a view reads, and it renders the date.
        let (start_ms, all_day, tz) = match &sf.when {
            When::Timed { start_ms, tz, .. } => (Some(*start_ms), false, tz.clone()),
            When::AllDay { start_date, .. } => (
                capture_time::FloatingDate::parse(start_date).and_then(|d| {
                    let z = capture_time::Zone::new(note.tz.as_deref().unwrap_or("UTC"))?;
                    capture_time::at_local(d, 0, 0, z)
                }),
                true,
                note.tz.clone().unwrap_or_else(|| String::from("UTC")),
            ),
        };

        if let (Some(start_ms), Some(zone)) = (start_ms, capture_time::Zone::new(&tz)) {
            let rule = to_rule(&sf.rule);
            let local = capture_time::local_of(start_ms, zone);
            let length = match &sf.when {
                When::Timed {
                    end_ms: Some(end), ..
                } => Some(end - start_ms),
                _ => None,
            };

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

            for o in capture_time::expand(&rule, local, zone, from, to) {
                let ex = note
                    .occurrences
                    .get(&o.start)
                    .or_else(|| inherited.and_then(|m| m.get(&o.start)));
                let (shown, end) = match ex.and_then(|e| e.moved_to) {
                    Some((s, e)) => (s, e),
                    // Each occurrence as long as the first: the length was
                    // written once, for all of them.
                    None => (o.start, length.map(|l| o.start + l)),
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
                            capture_time::Shift::Gap => "gap",
                            capture_time::Shift::Ambiguous => "ambiguous",
                        }),
                        // `visible()`, not `cancelled`. A move beats a
                        // cancellation on the same occurrence (§6, and m2's
                        // task 3.4 implemented it) — a resurrected occurrence
                        // is visible and one gesture away, where a wrongly
                        // cancelled one is invisible and silently missed.
                        //
                        // That rule lived in `OccState::visible()` and was read
                        // by nothing but its own test. This column is what
                        // every surface filters on, so the rule was true in the
                        // fold and absent everywhere it mattered — and the
                        // comment on `occurrences_of` already claimed it.
                        ex.map(|e| !e.visible()).unwrap_or(false) as i64,
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
        capture_core::body::Promoted::Event(e) => match &e.when {
            capture_core::body::When::Timed {
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
                // The same rule as the expansion above: a move beats a cancel.
                // Both paths write this column and both have to agree, or an
                // occurrence changes its mind about being cancelled depending
                // on whether the rule still produces it.
                !state.visible() as i64,
                state.done as i64,
                state.title.clone(),
                state.location.clone(),
                state.notes.clone(),
            ],
        )?;
    }

    Ok(())
}

/// The stored rule, as `capture-time` expands it.
fn to_rule(r: &capture_core::body::RuleSpec) -> capture_time::Rule {
    use capture_time::{Freq, Rule, Weekday};

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
    i: &capture_core::fold::Intent,
) -> Result<()> {
    use capture_core::body::Cue;

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

/// How long this note is suppressed for, entity-scoped and inline-tag mutes
/// included.
///
/// The fold settles the other three scopes and hands entity ones over whole,
/// because deciding which notes name an entity needs the text and
/// `capture_parse` already depends on `capture-core`. This is where that rule
/// gets carried out — the one place that has both the mute and the mentions.
///
/// `muted_at` is why this can be done here rather than having to be done there.
/// Newest-wins inside the fold is iteration order, so without a recorded
/// timestamp an entity mute applied at this point would beat every note-scoped
/// mute simply by being later in the code.
fn muted_for(p: &Projection, n: &capture_core::fold::Note) -> Option<u64> {
    let Some(text) = n.text.as_deref() else {
        // No readable text names nobody, so no entity mute can reach it — the
        // same guard that keeps a destroyed note off every entity page.
        return n.muted_until;
    };
    let named = entities_named(p, text);
    let inline_tags: Vec<String> = capture_parse::mentions(text)
        .into_iter()
        .filter(|(sigil, _)| *sigil == '#')
        .map(|(_, value)| value)
        .collect();
    if named.is_empty() && inline_tags.is_empty() {
        return n.muted_until;
    }

    let mut until = n.muted_until;
    let mut at = n.muted_at;
    for (ent, value, when) in &p.entity_mutes {
        if !named.contains(&p.entity_of(*ent)) {
            continue;
        }
        if at.is_none_or(|had| *when > had) {
            until = *value;
            at = Some(*when);
        }
    }
    // A tag written in the text, which the fold could not see. The same
    // newest-wins as everything else: a note muted by itself after the tag
    // was muted keeps its own mute, and the reverse.
    for (tag, value, when) in &p.tag_mutes {
        if !inline_tags.contains(tag) {
            continue;
        }
        if at.is_none_or(|had| *when > had) {
            until = *value;
            at = Some(*when);
        }
    }
    until
}

/// Work out which item each tick meant, and whether it is now done.
///
/// An item has no identity to find. Items are re-derived from a mutable string
/// on every fold, and nothing is minted at capture time to point at — the body
/// is frozen and carries no item ids. So this is a *reconciliation*, not a
/// lookup, and the question is only which failure is least bad.
///
/// Text alone is what this replaces: correcting a word discarded the tick,
/// which is the most ordinary edit there is. Position alone is worse in a
/// quieter way — inserting one item at the top shifts every position below it,
/// so every tick moves to the wrong row and nothing says so.
///
/// So both are weighed, in this order, and each rule is the answer to one edit:
///
/// 1. **Exact** — same position and same words. Nothing was touched.
/// 2. **The words, somewhere else** — they appear exactly once. Something was
///    inserted or removed above this item.
/// 3. **The position** — the words are gone but the slot is there. The item was
///    reworded.
/// 4. Otherwise the tick applies to nothing, and is left for a later fold where
///    the item may be back. An edit does not destroy a record of something that
///    happened.
///
/// Ticks are applied in event order, so a later one beats an earlier one on the
/// same item.
///
/// # What this gets wrong
///
/// Rewording an item *and* inserting another above it in one edit. Rule 1
/// misses, rule 2 misses because the words changed, and rule 3 fires on a
/// position now holding a different item — so the tick lands on the wrong row.
/// Rarer than either edit alone, and the alternative is to refuse every
/// ambiguous case, which would discard ticks for the common edits in order to
/// avoid being wrong in an uncommon one.
fn reconcile_ticks(
    written: Vec<bool>,
    items: &[String],
    ticks: &[capture_core::fold::ItemTick],
) -> Vec<bool> {
    // Where each item starts: unticked, or ticked as written (`[x]`, pages).
    let mut done = written;

    for tick in ticks {
        // 1. Exact.
        let at = match tick.ord.map(|o| o as usize) {
            Some(o) if o < items.len() && items[o] == tick.text => Some(o),
            _ => None,
        };

        // 2. The words, if they appear exactly once.
        let at = at.or_else(|| {
            let mut found = items.iter().enumerate().filter(|(_, i)| **i == tick.text);
            match (found.next(), found.next()) {
                (Some((o, _)), None) => Some(o),
                _ => None,
            }
        });

        // 3. The position — but only against the same number of items.
        //
        // The count is what makes this safe, and leaving it out is what the
        // first version got wrong: remove the middle of three and the position
        // still exists, holding the item that was below it, which then inherits
        // a tick nobody gave it. A test caught it.
        //
        // So a changed count means the list was added to or taken from, and a
        // position written against the old one means nothing. The tick then
        // matches nothing and applies to nothing — the event stays in the log,
        // and restoring the item restores it.
        let at = at.or_else(|| match (tick.ord.map(|o| o as usize), tick.of) {
            (Some(o), Some(of)) if of as usize == items.len() && o < items.len() => Some(o),
            _ => None,
        });

        if let Some(o) = at {
            done[o] = tick.done;
        }
    }

    done
}

/// Which kind of entity a sigil names.
///
/// `capture_core::id::entity_kind` decides, here and in the window, the
/// binding and the command line. It sits beside `EntId::derive`, which takes
/// the kind it returns — and this file is where that id is actually computed
/// and stored, so a second opinion here would mean entities filed under a kind
/// nobody wrote.
use capture_core::id::entity_kind;

/// The entities a note's text names, already resolved through any merges.
///
/// Computed from the text rather than read back from `mentions`, because the
/// caller needs it while deciding what to write about the note and the row is
/// not there yet.
fn entities_named(p: &Projection, text: &str) -> Vec<EntId> {
    let mut out = Vec::new();
    for (sigil, value) in capture_parse::mentions(text) {
        if let Some(kind) = entity_kind(sigil) {
            let ent = p.entity_of(EntId::derive(kind, &value));
            if !out.contains(&ent) {
                out.push(ent);
            }
        }
    }
    out
}

/// Recompute `entities` and `entity_names` from the mentions now stored.
///
/// Whole rather than per note, and that is not laziness. An entity's displayed
/// name, where nobody has chosen one, is the earliest spelling written for it
/// anywhere in the archive — so a single note arriving can change what an entity
/// is called, and a partial update would leave the answer depending on which
/// notes happened to be re-folded.
///
/// Derived twice over: from `mentions`, which is derived from note text, and
/// from the fold's entity state. Nothing here is truth, which is why deleting
/// both tables and rebuilding them is safe at any point.
fn write_entities(tx: &rusqlite::Transaction<'_>, p: &Projection) -> Result<()> {
    tx.execute_batch("DELETE FROM entities; DELETE FROM entity_names;")?;

    // Earliest first, so the first spelling seen for a name wins the display and
    // every later one is ignored by `INSERT OR IGNORE`. Ordered by the same
    // `(created, tgt)` the fold uses, so two devices choose the same spelling.
    let mut st = tx.prepare(
        "SELECT m.sigil, m.value
           FROM mentions m JOIN notes n ON n.tgt = m.tgt
          ORDER BY n.created_wall ASC, n.created_counter ASC, m.tgt ASC, m.value ASC",
    )?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;

    // ent_id -> the display to use, and the kind it came from.
    let mut display: BTreeMap<EntId, (String, String)> = BTreeMap::new();
    // Every written spelling, so a lookup by either reaches the entity.
    let mut names: Vec<(String, String, EntId)> = Vec::new();

    for row in rows {
        let (sigil, value) = row?;
        let Some(kind) = sigil.chars().next().and_then(entity_kind) else {
            continue;
        };
        let ent = p.entity_of(EntId::derive(kind, &value));
        // The spelling points at the entity it *ends up* being, not the one it
        // derives. A merge is followed here, once, rather than at every read —
        // and `entities` only ever holds the survivors, so a lookup that landed
        // on a merged-away id would find no row at all.
        names.push((kind.to_string(), casefold(&value), ent));
        display
            .entry(ent)
            .or_insert_with(|| (value.clone(), kind.to_string()));
    }
    drop(st);

    for (ent, (shown, kind)) in &display {
        // A name somebody chose beats the earliest spelling. It is the one thing
        // here that is not derived from the text.
        let shown = p.entity_name(*ent).unwrap_or(shown.as_str());
        tx.execute(
            "INSERT OR REPLACE INTO entities (ent_id, kind, display, note_tgt, merged_into)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                ent.0.to_vec(),
                kind,
                shown,
                p.entity_backing_of(*ent).map(|n| n.0.to_vec()),
                // Always null on a surviving row: a merge edge points *away*
                // from an entity, and only the entity at the end of the chain
                // gets a row here. The column is §1.4's, kept so the schema
                // matches the design it came from.
                None::<Vec<u8>>,
            ],
        )?;
    }

    for (kind, name, raw) in &names {
        tx.execute(
            "INSERT OR IGNORE INTO entity_names (kind, name, ent_id) VALUES (?1,?2,?3)",
            params![kind, name, raw.0.to_vec()],
        )?;
    }

    // An entity that only exists because somebody renamed or merged it — every
    // note naming it has since been edited or destroyed — still gets a row, so
    // that the rename is not silently lost if a note names it again later.
    for ent in p.entity_display.keys().chain(p.entity_backing.keys()) {
        let ent = p.entity_of(*ent);
        if display.contains_key(&ent) {
            continue;
        }
        if let Some(shown) = p.entity_name(ent) {
            tx.execute(
                "INSERT OR IGNORE INTO entities (ent_id, kind, display, note_tgt, merged_into)
                 VALUES (?1,?2,?3,?4,?5)",
                params![
                    ent.0.to_vec(),
                    "person",
                    shown,
                    p.entity_backing_of(ent).map(|n| n.0.to_vec()),
                    None::<Vec<u8>>,
                ],
            )?;
        }
    }

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

        if let Some(redact) = &n.cancelled_too_late {
            tx.execute(
                "INSERT OR REPLACE INTO late_cancels (tgt, redact) VALUES (?1,?2)",
                params![tgt.0.to_vec(), redact.0.to_vec()],
            )?;
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
                muted_for(p, n).map(|v| v as i64),
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

        // Links, withheld on the same condition as the mentions below.
        //
        // `links.name` is the words inside `[[..]]` — the person's own text,
        // and it leaks from both ends: the row names what a note referred to,
        // and the target's backlinks name the note that referred to it. This
        // loop had no guard of any kind. Destruction was covered by two
        // accidents rather than by the rule — the fold rebuilds a version's
        // links from its body, so a destroyed note has nothing to extract
        // from, and `purge_note` deletes the rows anyway — which left exactly
        // the ordered window uncovered.
        if n.readable_text().is_some() {
            for (i, l) in n.links.iter().enumerate() {
                tx.execute(
                    "INSERT OR REPLACE INTO links (src, ord, name, resolved) VALUES (?1,?2,?3,?4)",
                    params![t, i as i64, l.name, l.resolved.map(|x| x.0.to_vec())],
                )?;
            }
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

        // The items a note had are cleared whether or not it may be read.
        //
        // Above the guard, not inside it: withholding is about what gets
        // written back, and a note that already had items must *lose* them when
        // its destruction is ordered rather than keep the ones it had. Inside
        // the guard this skipped the delete too, so the withholding worked on a
        // note captured and queued in one breath and leaked on every real one.
        tx.execute("DELETE FROM list_items WHERE tgt = ?1", params![t])?;
        tx.execute("DELETE FROM unclear_days WHERE tgt = ?1", params![t])?;

        // Everything derived from the words, under one condition.
        //
        // `readable_text` rather than `text`, and `text_for_purge` deliberately
        // not used: a note whose content is withheld or destroyed must not go
        // on naming people or listing its contents, or an entity page and a
        // list become ways to read what redaction was supposed to have removed.
        //
        // `text.is_some()` was the old test and it is a test for the wrong
        // half: the fold keeps the text through the cancellation window so a
        // cancellation can restore it, so that condition stayed true for the
        // whole thirty minutes after somebody asked for the note to be gone.
        if let Some(text) = n.readable_text() {
            for (sigil, value) in capture_parse::mentions(text) {
                tx.execute(
                    "INSERT OR IGNORE INTO mentions (tgt, sigil, value) VALUES (?1,?2,?3)",
                    params![t, sigil.to_string(), value],
                )?;
            }

            // A page is never read (pages, 2026-10-13): no question about its
            // day, and its list is its checklist lines rather than the comma
            // form a capture uses.
            let page = matches!(
                n.promotion.as_ref().map(|p| &p.as_),
                Some(capture_core::body::Promoted::Page)
            );

            // UTC where no zone was recorded, as the sweep reads it.
            let zone =
                n.tz.as_deref()
                    .and_then(capture_time::Zone::new)
                    .or_else(|| capture_time::Zone::new("UTC"));
            if !page
                && let Some(zone) = zone
                && let Some(u) = capture_parse::unclear_day(text, n.created.wall as i64, zone)
            {
                tx.execute(
                    "INSERT OR REPLACE INTO unclear_days (tgt, first, second) VALUES (?1,?2,?3)",
                    params![t, u.choices[0].to_string(), u.choices[1].to_string()],
                )?;
            }

            // A list's items, expanded. Under the same guard as mentions and
            // for the same reason.
            //
            // The rows are replaced wholesale rather than merged, so an edit
            // that shortens a list does not leave the items it dropped behind —
            // which is why the delete is unconditional and sits above this
            // guard rather than in it.
            //
            // A page's checklist lines go in the same rows, untagged, so a tick
            // on one is the item tick every surface already draws. A line
            // written `[x]` starts ticked; a tick made on the page overrides it.
            let list = if page {
                let lines = capture_parse::checklist_of(text);
                let written: Vec<bool> = lines.iter().map(|l| l.done).collect();
                let items: Vec<String> = lines.into_iter().map(|l| l.item).collect();
                Some((String::new(), items, written))
            } else {
                capture_parse::list_of(text).map(|l| {
                    let none = vec![false; l.items.len()];
                    (l.tag, l.items, none)
                })
            };
            if let Some((tag, items, written)) = list {
                let done = reconcile_ticks(written, &items, &n.item_ticks);
                for (ord, item) in items.iter().enumerate() {
                    tx.execute(
                        "INSERT OR REPLACE INTO list_items (tgt, ord, tag, item, done)
                         VALUES (?1,?2,?3,?4,?5)",
                        params![t, ord as i64, tag, item, done[ord] as i64],
                    )?;
                }
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
        if let Some(text) = n.readable_text() {
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

    // Last, because it reads the `mentions` rows the loop above just wrote.
    //
    // Whole rather than per note: an entity's displayed name is the earliest
    // spelling written for it anywhere, so one arriving note can change what an
    // entity is called.
    write_entities(tx, p)?;

    Ok(())
}

/// The current generation, open for reading.
///
/// Written out by hand at fourteen call sites before this, as
///
/// ```ignore
/// Store::open_read(&generation_path(&paths.state, Ops::open_read(&paths.ops)?.generation()?))
/// ```
///
/// It is not merely long. It encodes a rule — **the current generation is
/// whatever `ops` says it is** — and a call site that opened a path it had
/// worked out some other way would read an archive that is real, internally
/// consistent, and out of date. That is the hardest kind of wrong to notice,
/// because nothing about it looks broken.
///
/// Callers that want a *specific* generation still say so: `rebuild` writes
/// one and the crash tests read the one from before a failure, and neither
/// should go through here.
pub fn current(paths: &crate::StorePaths) -> Result<Store> {
    let store = open_current(paths)?;
    // Refused by name rather than read. A projection built by an older build
    // lacks whatever tables were added since, and the first query against one
    // failed as "storage: no such table: late_cancels" — on the real archive,
    // the first time it was read after an install. A read cannot rebuild it
    // (§8.8 keeps reads off the writer), so the most it can do is say what
    // will.
    let found = store.schema_version()?;
    if found != PROJECTION_SCHEMA {
        return Err(crate::Error::Stale { found });
    }
    Ok(store)
}

/// The current generation, whatever shape it is.
///
/// For the caller that is about to ask whether it is stale and rebuild it if
/// so. Everything else wants [`current`], which refuses a stale one.
pub fn open_current(paths: &crate::StorePaths) -> Result<Store> {
    let generation = crate::ops::Ops::open_read(&paths.ops)?.generation()?;
    Store::open_read(&generation_path(&paths.state, generation))
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
pub fn rebuild(
    base: &Path,
    events: &[Envelope],
    aside: &[crate::log::AsideRow],
    current: u64,
    now: i64,
) -> Result<u64> {
    let next = current + 1;
    let path = generation_path(base, next);
    let _ = std::fs::remove_file(&path);

    let p = capture_core::fold::fold(events);
    let mut store = Store::open(&path)?;
    store.write(&p, now)?;

    // What this device is refusing to use, restored from the log.
    //
    // `quarantine` is the *report* of the log's `aside` column, and it was
    // written only by the gate at ingest — so a rebuild produced a projection
    // with no report at all. The events stayed set aside, invisible to every
    // surface, and `Gate::reconsider` had nothing to re-admit when a retirement
    // was reissued. That remedy was made local precisely so it would not depend
    // on a peer still holding them, and a rebuild silently put the dependency
    // back.
    //
    // Here rather than in the caller because every producer of a projection has
    // to produce a complete one. "The caller remembers" is the arrangement that
    // caused this.
    for row in aside {
        store.quarantine(&row.id, &row.dev, row.seq, &row.reason, row.at)?;
    }

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

/// Record how far a generation has folded, for the incremental path.
///
/// `rebuild` has always written this; the incremental fold never did, so the
/// one comparison that could tell a projection from the log was only ever
/// correct immediately after a full rebuild.
pub fn note_folded(store: &Store, events: &[Envelope]) -> Result<()> {
    let mut per: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    for e in events {
        let cur = per
            .entry(e.header.dev.0.iter().map(|b| format!("{b:02x}")).collect())
            .or_default();
        *cur = (*cur).max(e.header.seq);
    }
    for (d, s) in per {
        if store.fold_head(&d)? < s {
            store.set_fold_head(&d, s)?;
        }
    }
    Ok(())
}

/// Which generations exist on disk, in order.
///
/// The paths are built from a number and were never enumerated, so nothing
/// could ask what had accumulated — which is how every superseded generation
/// came to be kept for ever. `rebuild_projection` writes beside the current one
/// and moves the pointer, correctly, and nothing ever came back for the file it
/// replaced.
///
/// Only names this module writes. A directory holding something else keeps it.
pub fn generations(base: &Path) -> Result<Vec<u64>> {
    let dir = base.parent().unwrap_or(Path::new("."));
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(out);
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        // Exactly `state.<n>.db`, so a sidecar (`-wal`, `-shm`) and anything
        // else sharing the directory are left alone.
        if let Some(rest) = name.strip_prefix("state.")
            && let Some(num) = rest.strip_suffix(".db")
            && let Ok(n) = num.parse::<u64>()
        {
            out.push(n);
        }
    }
    out.sort_unstable();
    Ok(out)
}

/// Remove every generation older than the one before `current`.
///
/// Returns how many were removed.
///
/// # Why one spare is kept rather than none
///
/// A reader does two things that are not one step: it asks `ops` which
/// generation is current, and it opens that file. §8.8 keeps reads off the
/// writer lock, so they cannot be made atomic — and that is the right trade,
/// because a read that needs the writer is a read that fails when the writer
/// does.
///
/// Between those two steps a generation may be superseded. Deleting the file
/// the moment it stops being current turns that window into a failed read.
/// Keeping one spare closes it for any realistic reader: to be caught, a read
/// would have to resolve generation `n` and then be delayed across *two*
/// rebuilds before opening it, and a rebuild is a full re-fold of the log.
pub fn discard_superseded(base: &Path, current: u64) -> Result<usize> {
    let keep = current.saturating_sub(1);
    let mut n = 0;
    for g in generations(base)? {
        if g < keep {
            discard_generation(base, g)?;
            n += 1;
        }
    }
    Ok(n)
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

/// The same, for an entity id.
fn ent_from(b: Vec<u8>) -> Option<EntId> {
    <[u8; 32]>::try_from(b.as_slice()).ok().map(EntId)
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
    use capture_core::testing;

    fn base(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("capture-proj-{name}-{}", std::process::id()));
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
        let p = capture_core::fold::fold(&s.events);
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
        let g1 = rebuild(&b, &s.events, &[], 0, 1_800_000_000_000).unwrap();
        let g2 = rebuild(&b, &s.events, &[], g1, 1_800_000_000_000).unwrap();
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
        let g = rebuild(&b, &s.events, &[], 0, 1_800_000_000_000).unwrap();
        let a = Store::open_read(&generation_path(&b, g)).unwrap();
        let n1 = a.note_count().unwrap();

        let g2 = rebuild(&b, &s.events, &[], g, 1_800_000_000_000).unwrap();
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
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "the deploy keeps timing out on staging"),
            1_700_000_000_000,
        );
        let g = rebuild(&b, std::slice::from_ref(&e), &[], 0, 1_800_000_000_000).unwrap();
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
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "a password nobody should still be able to read"),
            1_700_000_000_000,
        );
        let tgt = cap.header.id();
        let redact = d.author(
            capture_core::Kind::REDACT,
            Some(tgt),
            None,
            testing::encode(&capture_core::body::RedactBody {
                nonce: [0u8; 16],
                scope: capture_core::body::RedactScope::WholeNote,
                targets: vec![tgt],
            }),
            1_700_000_001_000,
        );

        let g = rebuild(&b, &[cap, redact], &[], 0, 1_800_000_000_000).unwrap();
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
            capture_core::Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "original wording"),
            1_700_000_000_000,
        );
        let tgt = cap.header.id();
        let edit = d.author(
            capture_core::Kind::EDIT,
            Some(tgt),
            Some(tgt),
            testing::edit_body(2, "replacement wording with pangolin in it"),
            1_700_000_001_000,
        );
        let g = rebuild(&b, &[cap, edit], &[], 0, 1_800_000_000_000).unwrap();
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
        use capture_core::body::{RedactBody, RedactScope};
        let b = base("search-redact");
        let mut d = testing::Origin::new(0);
        let cap = d.author(
            capture_core::Kind::CAPTURE,
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
            capture_core::Kind::REDACT,
            Some(tgt),
            None,
            body,
            1_700_000_002_000,
        );

        let g = rebuild(&b, &[cap, red], &[], 0, 1_800_000_000_000).unwrap();
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
        let g = rebuild(&b, &s.events, &[], 0, 1_800_000_000_000).unwrap();
        let store = Store::open_read(&generation_path(&b, g)).unwrap();
        assert!(store.held_count().unwrap() > 0);
        clean(&b);
    }

    #[test]
    fn discarding_a_generation_removes_its_sidecars() {
        let b = base("discard");
        let s = testing::mixed(5, 1, 1, 2);
        let g = rebuild(&b, &s.events, &[], 0, 1_800_000_000_000).unwrap();
        discard_generation(&b, g).unwrap();
        assert!(!generation_path(&b, g).exists());
        clean(&b);
    }

    fn alloc_vec<T: Clone>(v: &[T]) -> Vec<T> {
        v.to_vec()
    }
}
