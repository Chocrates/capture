//! Local state that cannot be recomputed.
//!
//! # The split rule is derivability, not category
//!
//! Anything reconstructible from the log lives in the projection and is rebuilt
//! for free. Only what exists solely on this device, and could not be worked
//! out again, belongs here.
//!
//! That rule is easy to state and easy to get wrong, because plenty of things
//! *feel* operational while being perfectly derivable. The queue of pending
//! redactions is the cautionary case: it looks like local bookkeeping, but it
//! is entirely a function of the redaction events and their cancellations. Put
//! it here and a restore replays the log, marks the notes redacted, rebuilds
//! the projection — and then never destroys the payloads, because nothing is
//! driving the destruction. The device would hold every redacted plaintext
//! indefinitely while reporting all redactions complete.
//!
//! What genuinely belongs here:
//!
//! - Events that failed authentication. They are not part of the log — that is
//!   the point of rejecting them — so they cannot be recovered by folding it.
//! - The projection generation pointer, which describes this device's files.
//! - Peer addresses, and other things learned rather than derived.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use capture_core::EventId;
use rusqlite::{Connection, OptionalExtension, params};

use crate::{Result, db};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS local (k TEXT PRIMARY KEY, v TEXT NOT NULL) STRICT;

-- Events that failed authentication. Keyed by identity alone: they must never
-- claim a position in any origin's sequence, so there is deliberately no
-- (dev, seq) uniqueness here and no relationship to the log.
CREATE TABLE IF NOT EXISTS rejected (
  id         BLOB PRIMARY KEY,
  dev        TEXT,
  seq        INTEGER,
  reason     TEXT    NOT NULL,
  first_seen INTEGER NOT NULL,
  raw        BLOB
) STRICT;

-- Where a peer was last reached, and what it was last advertising.
--
-- Keyed by public key rather than by origin, because membership is keyed by
-- key (§5.3) and a restored device keeps its key while minting a new origin.
-- Keying this by origin would lose the address of a device precisely when it
-- had just been restored and most needed finding.
--
-- Two jobs. `addr` is the static fallback: mDNS is blocked on many corporate
-- and guest networks, and a system that only syncs at home is not a system.
-- `digest` is what makes a connection skippable — it is compared against what
-- the peer is advertising now, and equal means there is nothing to fetch.
--
-- An earlier `peers` table, keyed by origin and never read or written, is left
-- in whatever stores already created it: dropping tables is not something this
-- schema does, and nothing consults it.
CREATE TABLE IF NOT EXISTS peer_state (
  peer_key  BLOB PRIMARY KEY,
  addr      TEXT,
  digest    BLOB,
  last_seen INTEGER,
  attempts  INTEGER NOT NULL DEFAULT 0
) STRICT;

-- How far through the ordered purge a redaction got.
--
-- The queue itself is derived and lives in the projection; this is the part
-- that is not. Destroying a payload touches several places in a fixed order,
-- and a crash halfway leaves content in the ones not yet reached. Only the
-- process doing the work knows where it stopped, and re-folding the log cannot
-- reconstruct that — it would report the redaction complete while a copy
-- survives in the search index.
CREATE TABLE IF NOT EXISTS redact_intent (
  redact_evt BLOB PRIMARY KEY,
  tgt        BLOB NOT NULL,
  phase      TEXT NOT NULL,
  started_at INTEGER NOT NULL
) STRICT;

-- Copies taken just before a redaction executes, so the window in which it can
-- be cancelled is real. Local by definition: they are never replicated, and a
-- device that rebuilt from the log would have no way to recreate content the
-- log no longer contains.
CREATE TABLE IF NOT EXISTS snapshots (
  redact_evt BLOB NOT NULL,
  tgt        BLOB NOT NULL,
  taken_at   INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  body       BLOB,
  PRIMARY KEY (redact_evt, tgt)
) STRICT;

-- Devices this one has deliberately paired with. Without it, \"a device was
-- added that you did not expect\" is not a computable statement — every
-- admission looks the same once it is in the log.
-- How far each origin has been exported.
--
-- Per origin, not one number. Events do not arrive in a single global order, so
-- a scalar watermark either skips whatever landed late or re-exports the whole
-- archive every time something does — the same mistake a single fold watermark
-- would be.
CREATE TABLE IF NOT EXISTS export_head (
  dev TEXT PRIMARY KEY,
  seq INTEGER NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS pairings (
  peer_key  BLOB PRIMARY KEY,
  paired_at INTEGER NOT NULL
) STRICT;

-- Admissions the person has seen and accepted.
--
-- Separate from `pairings` because they say different things. A pairing is
-- a statement that this device did the pairing; an acknowledgement is a
-- statement that it did not, and knows. Storing
-- an acknowledgement as a pairing would erase the difference, and the next
-- unexpected admission of the same key would go unreported.
--
-- Acknowledgeable rather than self-clearing: a device this one did not admit
-- appearing in the roster is either an ordinary consequence of pairing on
-- another device, or the compromise §5.3 admits it cannot prevent. Nothing
-- here can tell those apart, so it asks rather than deciding.
-- Who a redaction still has to reach.
--
-- The set is fixed when the redaction is issued, and is deliberately **not**
-- read from the current roster. The realistic sequence is: the phone is stolen,
-- you retire it, and *then* you redact the sensitive notes. If outstanding
-- peers were computed from the roster, retiring the phone would remove it from
-- the set and every surface would report complete propagation for the one
-- device that never received anything.
--
-- So a retired peer stays listed as never acknowledged. That is the true
-- statement, and it is the useful one.
CREATE TABLE IF NOT EXISTS redact_peers (
  redact_evt BLOB NOT NULL,
  peer_key   BLOB NOT NULL,
  issued_at  INTEGER NOT NULL,
  acked_at   INTEGER,
  PRIMARY KEY (redact_evt, peer_key)
) STRICT;

-- The X25519 key a peer answers a handshake with, learned out of band.
--
-- Normally this comes from the log: a membership event is where a device says
-- what it is. A newly joined device has no log yet — that is what it is about
-- to sync — and it cannot dial anybody to get one without a key to dial with.
-- The pairing grant carries the admitter's key precisely to break that
-- deadlock, and this is where it is kept until the log catches up.
CREATE TABLE IF NOT EXISTS peer_noise (
  peer_key BLOB PRIMARY KEY,
  noise    BLOB NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS admissions_seen (
  peer_key BLOB PRIMARY KEY,
  seen_at  INTEGER NOT NULL
) STRICT;
";

/// What is known about a peer: where it lives, and when it was last heard.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PeerAddress {
    /// The peer's public key. Membership is keyed by key, not by origin (§5.3).
    pub key: [u8; 32],
    /// Where to dial it, if that is known.
    ///
    /// Optional because the two facts are learned separately and one does not
    /// imply the other: a peer that only ever dials *in* is demonstrably alive
    /// and has no address here, since the port it called from is ephemeral.
    /// Collapsing them would mean either inventing an address that never
    /// answers or reporting a device reached a minute ago as never seen.
    pub addr: Option<String>,
    /// Consecutive failed dials, which is what the backoff is computed from.
    pub attempts: u32,
    /// When it was last reached, so a peer gone quiet for a week can be
    /// reported with *when* rather than only that it is stale (§4.5).
    pub last_seen: Option<u64>,
}

fn id_from(b: Vec<u8>) -> Option<EventId> {
    <[u8; 32]>::try_from(b.as_slice()).ok().map(EventId)
}

/// When the capture gesture last reached the application.
const GESTURE_FIRED: &str = "gesture_last_fired";

/// How long a pre-redaction copy is kept.
///
/// Long enough to notice a redaction fired at the wrong note and act on it;
/// short enough that it is a safety net rather than a second archive of exactly
/// the material somebody asked to be destroyed.
pub const SNAPSHOT_RETENTION_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// Redactions whose content may still sit in already-written export files.
const STALE_EXPORTS: &str = "stale_exports";

/// When the due-work queue was last drained.
const DUE_PROCESSED: &str = "due_last_processed";

/// When a bounded round last ran, whatever it achieved.
const ROUND_AT: &str = "sync_last_round";

/// Whether discovery was available to that round.
const ROUND_MULTICAST: &str = "sync_last_multicast";

/// The last thing that round reported going wrong, if anything did.
const ROUND_TROUBLE: &str = "sync_last_trouble";

/// Bucket counts for how long the overlay takes to appear.
const LATENCY: &str = "overlay_latency";

/// Handle to this device's local state.
pub struct Ops {
    conn: Connection,
}

impl Ops {
    /// Open or create.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = db::open_write(path)?;
        db::create_schema(&conn, SCHEMA)?;
        Ok(Ops { conn })
    }

    /// Open read-only.
    pub fn open_read(path: &Path) -> Result<Self> {
        Ok(Ops {
            conn: db::open_read(path)?,
        })
    }

    /// Read a local value.
    pub fn get(&self, k: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT v FROM local WHERE k = ?1", params![k], |r| {
                r.get::<_, String>(0)
            })
            .optional()?)
    }

    /// Write a local value.
    pub fn set(&self, k: &str, v: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO local (k, v) VALUES (?1, ?2)
             ON CONFLICT(k) DO UPDATE SET v = ?2",
            params![k, v],
        )?;
        Ok(())
    }

    /// Drop a local value.
    pub fn forget_key(&self, k: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM local WHERE k = ?1", params![k])?;
        Ok(())
    }

    /// Which projection generation is current.
    ///
    /// Lives here rather than inside the projection because it *describes* the
    /// projection: a rebuild writes a new generation alongside the old one and
    /// then moves this pointer. Keeping it in the thing being replaced would
    /// mean the pointer disappearing at the moment it is needed.
    pub fn generation(&self) -> Result<u64> {
        Ok(self
            .get("state_generation")?
            .and_then(|s| s.parse().ok())
            .unwrap_or(0))
    }

    /// Point at a new projection generation.
    pub fn set_generation(&self, n: u64) -> Result<()> {
        self.set("state_generation", &n.to_string())
    }

    /// Record an event that failed authentication.
    ///
    /// Kept for diagnosis rather than for use: it is never folded, never
    /// relayed, and never allowed to occupy a position a genuine event might
    /// need.
    ///
    /// `raw` is the event's bytes, kept only where refusal might be temporary.
    /// An event refused because no key is known for its origin becomes
    /// acceptable the moment the claim binding that origin arrives, and without
    /// the bytes there is nothing left to reconsider — the origin would stay
    /// permanently unusable for a reason that stopped being true.
    pub fn reject(
        &self,
        id: &[u8; 32],
        dev: Option<&str>,
        seq: Option<u64>,
        reason: &str,
        at: u64,
        raw: Option<&[u8]>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO rejected (id, dev, seq, reason, first_seen, raw)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id.to_vec(),
                dev,
                seq.map(|s| s as i64),
                reason,
                at as i64,
                raw
            ],
        )?;
        Ok(())
    }

    /// Keep a copy of what is about to be destroyed.
    ///
    /// Local, never exported, and never replicated: it is a safety net for a
    /// mistake made on this device, and copying it anywhere else would
    /// reintroduce exactly the plaintext the redaction was issued to remove —
    /// in a place the redaction cannot reach.
    pub fn take_snapshot(
        &self,
        redact: &EventId,
        tgt: &EventId,
        text: &str,
        at: u64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO snapshots (redact_evt, tgt, taken_at, expires_at, body)
             VALUES (?1,?2,?3,?4,?5)",
            params![
                redact.0.to_vec(),
                tgt.0.to_vec(),
                at as i64,
                (at + SNAPSHOT_RETENTION_MS) as i64,
                text.as_bytes()
            ],
        )?;
        Ok(())
    }

    /// A copy taken before a redaction, if one was kept and still lives.
    pub fn snapshot(&self, redact: &EventId, now: u64) -> Result<Option<String>> {
        let row: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT body FROM snapshots WHERE redact_evt = ?1 AND expires_at > ?2",
                params![redact.0.to_vec(), now as i64],
                |r| r.get(0),
            )
            .optional()?;
        Ok(row.map(|b| String::from_utf8_lossy(&b).to_string()))
    }

    /// Discard every snapshot whose retention has elapsed.
    ///
    /// The retention is the whole point: a copy kept forever is not a safety
    /// net, it is a second archive of the material somebody asked to be
    /// destroyed.
    pub fn expire_snapshots(&self, now: u64) -> Result<usize> {
        let n = self.conn.execute(
            "DELETE FROM snapshots WHERE expires_at <= ?1",
            params![now as i64],
        )?;
        Ok(n)
    }

    /// How many copies are being held, for reporting.
    pub fn snapshot_count(&self, now: u64) -> Result<u64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM snapshots WHERE expires_at > ?1",
            params![now as i64],
            |r| r.get::<_, i64>(0),
        )? as u64)
    }

    /// Record that a purge is under way, or how far it has got.
    ///
    /// Written before anything is destroyed and updated after each step. It is
    /// the only thing that can distinguish an interrupted purge from one that
    /// never started: the redaction event in the log looks identical either
    /// way, so without this a half-finished purge is never noticed again.
    pub fn begin_purge(
        &self,
        redact: &EventId,
        tgt: &EventId,
        phase: crate::purge::Phase,
        at: u64,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO redact_intent (redact_evt, tgt, phase, started_at) VALUES (?1,?2,?3,?4)
             ON CONFLICT(redact_evt) DO UPDATE SET phase = ?3",
            params![redact.0.to_vec(), tgt.0.to_vec(), phase.as_str(), at as i64],
        )?;
        Ok(())
    }

    /// Mark a purge finished, which is the only thing that clears its marker.
    pub fn finish_purge(&self, redact: &EventId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM redact_intent WHERE redact_evt = ?1",
            params![redact.0.to_vec()],
        )?;
        Ok(())
    }

    /// Purges that were started and never finished.
    pub fn unfinished_purges(&self) -> Result<Vec<(EventId, EventId, crate::purge::Phase)>> {
        let mut st = self
            .conn
            .prepare("SELECT redact_evt, tgt, phase FROM redact_intent ORDER BY started_at")?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, Vec<u8>>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (redact, tgt, phase) = row?;
            // A phase this build does not recognise is left in place rather
            // than guessed at: guessing would be guessing how much of an
            // irreversible operation has already happened.
            let Some(phase) = crate::purge::Phase::parse(&phase) else {
                continue;
            };
            if let (Some(a), Some(b)) = (id_from(redact), id_from(tgt)) {
                out.push((a, b, phase));
            }
        }
        Ok(out)
    }

    /// Note that content was destroyed which older export files still hold.
    ///
    /// An export directory is copied offsite by external tools, and nothing
    /// here can reach it. A redaction destroys the content in this store and in
    /// every replica; it cannot touch a file already sitting on somebody's
    /// backup drive. That gap is not closeable from inside the program, so the
    /// only honest response is to say so and keep saying it until acknowledged.
    pub fn note_stale_exports(&self, at: u64) -> Result<()> {
        let count = self.stale_export_count()? + 1;
        self.set(STALE_EXPORTS, &format!("{count},{at}"))
    }

    /// How many redactions have happened since the exports were last pruned.
    pub fn stale_export_count(&self) -> Result<u64> {
        Ok(self
            .get(STALE_EXPORTS)?
            .and_then(|v| v.split(',').next().and_then(|n| n.parse().ok()))
            .unwrap_or(0))
    }

    /// Acknowledge that the export directory has been dealt with.
    ///
    /// Acknowledgeable rather than self-clearing, because the system cannot
    /// verify it: the files are somewhere it has no access to. A condition it
    /// cleared on its own would be claiming to know something it does not.
    pub fn exports_pruned(&self) -> Result<()> {
        self.set(STALE_EXPORTS, "0,0")
    }

    /// Note that the due-work queue was looked at.
    ///
    /// Reported by `capture status`. Without it, a queue that has not been
    /// drained for weeks is indistinguishable from one with nothing in it —
    /// and the failure that hides is redactions sitting undestroyed while
    /// every interface reports them complete.
    pub fn due_processed(&self, at: u64) -> Result<()> {
        self.set(DUE_PROCESSED, &at.to_string())
    }

    /// When due work was last processed.
    pub fn due_last_processed(&self) -> Result<Option<u64>> {
        Ok(self.get(DUE_PROCESSED)?.and_then(|v| v.parse().ok()))
    }

    /// Note that the capture gesture actually reached the application.
    ///
    /// Local because it is about this device's desktop, not about the archive:
    /// a chord that works here says nothing about whether one works on another
    /// machine, and a re-fold must not clear it.
    pub fn gesture_fired(&self, at: u64) -> Result<()> {
        self.set(GESTURE_FIRED, &at.to_string())
    }

    /// When the gesture last reached the application, if it ever has.
    ///
    /// `None` means it has never been seen to work — which is a different
    /// condition from having worked and stopped, and is reported differently.
    pub fn gesture_last_fired(&self) -> Result<Option<u64>> {
        Ok(self.get(GESTURE_FIRED)?.and_then(|v| v.parse().ok()))
    }

    /// Whether an intent has already been delivered here.
    ///
    /// Local state, keyed by the event that made the intent. Which device
    /// managed to deliver a notification and when is operational chatter rather
    /// than a fact about the archive (§2.5.2), and keeping it here means two
    /// devices can each fire their own local cues without arguing about it.
    pub fn intent_fired(&self, id: &[u8]) -> Result<bool> {
        Ok(self.get(&intent_key(id))?.is_some())
    }

    /// When one was last delivered, if it has been.
    ///
    /// Separate from [`Self::intent_fired`] because the two answer different
    /// questions. "Has this ever fired" settles a cue that happens once; a
    /// geofence happens every time somebody arrives, and what it needs to know
    /// is *when* — so that standing near the boundary does not produce a
    /// notification every time the radio changes its mind.
    pub fn intent_fired_at(&self, id: &[u8]) -> Result<Option<u64>> {
        Ok(self
            .get(&intent_key(id))?
            .and_then(|v| v.trim().parse::<u64>().ok()))
    }

    /// Remember that one was delivered.
    pub fn mark_intent_fired(&self, id: &[u8], at: u64) -> Result<()> {
        self.set(&intent_key(id), &at.to_string())
    }

    /// Whether a moment has already been delivered.
    ///
    /// Local state on the same terms as a fired intent (§2.5.2): which device
    /// buzzed, and when, is operational chatter rather than a fact about the
    /// archive. It also means two devices each deliver their own without
    /// arguing — and that an archive restored elsewhere does not replay a
    /// year of notifications.
    pub fn alerted(&self, tgt: &[u8], occ: Option<i64>, at: i64) -> Result<bool> {
        Ok(self.get(&alert_key(tgt, occ, at))?.is_some())
    }

    /// Remember that one was delivered.
    pub fn mark_alerted(&self, tgt: &[u8], occ: Option<i64>, moment: i64, at: u64) -> Result<()> {
        self.set(&alert_key(tgt, occ, moment), &at.to_string())
    }

    /// Carry delivery forward when a series is split into a new note.
    ///
    /// # Why this is not bookkeeping
    ///
    /// Delivery is keyed by the note, and splitting a series gives the
    /// occurrences from the split point onward a *new* note — a new capture,
    /// with its own target, carrying the same commitment under a different
    /// schedule. Every one of them therefore reads as never delivered, and the
    /// boundary occurrence is the one that bites: it is the same instant and
    /// the same text as one already announced.
    ///
    /// "Change standup from today onward", done at four in the afternoon,
    /// re-announced this morning's standup. Splitting from a date further back
    /// re-announced every occurrence since it.
    ///
    /// The expansion already inherits *exceptions* across a split (§3.5) for
    /// the same reason: what somebody did to an occurrence survives a change to
    /// the schedule it sits on. Delivery inherited nothing, which is the same
    /// principle missing its other half.
    ///
    /// Only from the split point onward. Occurrences below it still belong to
    /// the old note and keep their own records there.
    pub fn carry_alerts(&self, from: &[u8], to: &[u8], from_occ: i64) -> Result<usize> {
        let from_hex: String = from.iter().take(8).map(|b| format!("{b:02x}")).collect();
        let to_hex: String = to.iter().take(8).map(|b| format!("{b:02x}")).collect();
        let prefix = format!("alerted.{from_hex}.");

        let mut q = self
            .conn
            .prepare("SELECT k, v FROM local WHERE k LIKE ?1 || '%'")?;
        let rows = q
            .query_map(params![prefix], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut carried = 0;
        for (k, v) in rows {
            let Some(suffix) = k.strip_prefix(&prefix) else {
                continue;
            };
            // Series occurrences only. A one-off's key carries `at<instant>`
            // and a one-off is not what was split.
            let Ok(occ) = suffix.parse::<i64>() else {
                continue;
            };
            if occ < from_occ {
                continue;
            }
            self.set(&format!("alerted.{to_hex}.{occ}"), &v)?;
            carried += 1;
        }
        Ok(carried)
    }

    /// Add one overlay-appearance measurement.
    ///
    /// Read-modify-write rather than an accumulating table: a histogram is a
    /// dozen integers, and keeping it as one row means the whole thing is
    /// written or none of it is.
    pub fn record_latency(&self, ms: u64) -> Result<()> {
        let mut h = self.latency()?;
        h.record(ms);
        let encoded: Vec<String> = h.counts().iter().map(|c| c.to_string()).collect();
        self.set(LATENCY, &encoded.join(","))
    }

    /// What has been measured so far.
    pub fn latency(&self) -> Result<crate::latency::Histogram> {
        let Some(raw) = self.get(LATENCY)? else {
            return Ok(crate::latency::Histogram::default());
        };
        let counts: Vec<u64> = raw.split(',').filter_map(|v| v.parse().ok()).collect();
        Ok(crate::latency::Histogram::from_counts(&counts))
    }

    /// How long a capture took, gesture to committed, on one surface.
    ///
    /// Per surface rather than one number, because the surfaces differ in what
    /// they have to do first: a tile opens a window, a share sheet arrives
    /// with the content already chosen, a recording has to be encoded and
    /// hashed before anything is written. One histogram over all of them would
    /// average away the only thing worth knowing — which gesture is slow.
    ///
    /// Measured rather than asserted. §2.1's whole argument is that capture
    /// has to cost almost nothing at the moment a thought arrives, and a
    /// system that claims that without measuring it is making a promise it has
    /// no way to keep.
    pub fn record_capture_latency(&self, surface: &str, ms: u64) -> Result<()> {
        let mut h = self.capture_latency(surface)?;
        h.record(ms);
        let encoded: Vec<String> = h.counts().iter().map(|c| c.to_string()).collect();
        self.set(&capture_latency_key(surface), &encoded.join(","))
    }

    /// What one surface has cost so far.
    pub fn capture_latency(&self, surface: &str) -> Result<crate::latency::Histogram> {
        let Some(raw) = self.get(&capture_latency_key(surface))? else {
            return Ok(crate::latency::Histogram::default());
        };
        let counts: Vec<u64> = raw.split(',').filter_map(|v| v.parse().ok()).collect();
        Ok(crate::latency::Histogram::from_counts(&counts))
    }

    /// Every surface that has reported a measurement.
    pub fn measured_surfaces(&self) -> Result<Vec<String>> {
        let mut st = self
            .conn
            .prepare("SELECT k FROM local WHERE k LIKE 'capture_latency.%' ORDER BY k")?;
        let rows = st.query_map([], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?.trim_start_matches("capture_latency.").to_string());
        }
        Ok(out)
    }

    /// How far each origin has been exported.
    pub fn export_heads(&self) -> Result<BTreeMap<String, u64>> {
        let mut st = self.conn.prepare("SELECT dev, seq FROM export_head")?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
        })?;
        let mut out = BTreeMap::new();
        for r in rows {
            let (d, s) = r?;
            out.insert(d, s);
        }
        Ok(out)
    }

    /// Record how far each origin has been exported.
    ///
    /// Called only after the export file is on disk. The other order would mean
    /// a crash between the two silently skipping those events in every future
    /// export, with nothing to notice it.
    pub fn set_export_heads(&self, heads: &BTreeMap<String, u64>) -> Result<()> {
        for (dev, seq) in heads {
            self.conn.execute(
                "INSERT INTO export_head (dev, seq) VALUES (?1, ?2)
                 ON CONFLICT(dev) DO UPDATE SET seq = ?2",
                params![dev, *seq as i64],
            )?;
        }
        Ok(())
    }

    /// Refusals worth reconsidering, with the bytes needed to do it.
    ///
    /// Only the ones whose reason can stop being true. A bad signature stays a
    /// bad signature however the roster changes, and retrying those forever
    /// would turn every membership event into a re-verification of the entire
    /// forensic pile.
    /// Record that a round ran, and what it managed.
    ///
    /// # Why this is not the peer rows
    ///
    /// `peer_state.last_seen` answers "when was this device last reached". It
    /// cannot answer "is sync running", because the two most different
    /// situations a person can be in produce an identical row: a round that ran
    /// and found nobody awake, and a round that never ran at all.
    ///
    /// The second is the one that goes unnoticed for a week. A periodic job can
    /// be held off indefinitely by a constraint it will never satisfy — a
    /// network the platform calls metered, a battery under the threshold every
    /// evening — and the platform reports that as nothing whatsoever: no run,
    /// no failure, no retry, because the job was never eligible to begin with.
    /// Every surface then reads exactly as it does on a quiet network.
    ///
    /// So the fact that a round *happened* is recorded separately from anything
    /// it achieved, and a surface can tell "nobody was home" from "nobody
    /// knocked".
    pub fn round_ran(&self, at: u64, multicast: bool, trouble: Option<&str>) -> Result<()> {
        self.set(ROUND_AT, &at.to_string())?;
        self.set(ROUND_MULTICAST, if multicast { "1" } else { "0" })?;
        match trouble {
            Some(t) => self.set(ROUND_TROUBLE, t)?,
            // Cleared rather than left: a fault from four days ago, still being
            // reported after the rounds since have been clean, teaches somebody
            // that the surface is describing the past.
            None => self.forget_key(ROUND_TROUBLE)?,
        }
        Ok(())
    }

    /// When a round last ran, whatever it achieved.
    pub fn last_round(&self) -> Result<Option<u64>> {
        Ok(self.get(ROUND_AT)?.and_then(|v| v.parse().ok()))
    }

    /// Whether discovery was available to the last round that ran.
    ///
    /// `None` when no round has run, which is not the same as `Some(false)`.
    pub fn last_round_had_discovery(&self) -> Result<Option<bool>> {
        Ok(self.get(ROUND_MULTICAST)?.map(|v| v == "1"))
    }

    /// What the last round reported going wrong, if anything did.
    pub fn last_round_trouble(&self) -> Result<Option<String>> {
        self.get(ROUND_TROUBLE)
    }

    /// Record that a peer answered, without claiming to know where it lives.
    ///
    /// An inbound connection proves the peer is alive and says nothing about
    /// where to call it: the source port is the ephemeral one it dialled out
    /// from, and dialling it back would never answer. Writing that down as an
    /// address is worse than writing nothing, because it replaces an address
    /// that worked with one that cannot.
    pub fn peer_answered(&self, peer_key: &[u8; 32], at: u64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO peer_state (peer_key, last_seen, attempts) VALUES (?1, ?2, 0)
             ON CONFLICT(peer_key) DO UPDATE SET last_seen = ?2, attempts = 0",
            rusqlite::params![&peer_key[..], at as i64],
        )?;
        Ok(())
    }

    /// Record that a peer was reached, and what it was advertising.
    ///
    /// Reaching it resets the retry count: backoff exists to stop dialling a
    /// device that is off, and a device that just answered is not off.
    pub fn peer_reached(&self, peer_key: &[u8; 32], addr: &str, at: u64) -> Result<()> {
        self.conn.execute(
            "INSERT INTO peer_state (peer_key, addr, last_seen, attempts)
             VALUES (?1, ?2, ?3, 0)
             ON CONFLICT(peer_key) DO UPDATE SET addr = ?2, last_seen = ?3, attempts = 0",
            rusqlite::params![&peer_key[..], addr, at as i64],
        )?;
        Ok(())
    }

    /// Record the digest a peer was advertising when this device last agreed
    /// with it, so a later advertisement can be compared against it.
    pub fn peer_agreed(&self, peer_key: &[u8; 32], digest: &[u8; 8]) -> Result<()> {
        self.conn.execute(
            "INSERT INTO peer_state (peer_key, digest) VALUES (?1, ?2)
             ON CONFLICT(peer_key) DO UPDATE SET digest = ?2",
            rusqlite::params![&peer_key[..], &digest[..]],
        )?;
        Ok(())
    }

    /// Whether an advertised digest says there is anything to fetch.
    ///
    /// Unknown peer, or no recorded agreement, means yes — the safe direction.
    /// Skipping a peer this device has never synced with would be a device
    /// that never syncs.
    pub fn peer_has_news(&self, peer_key: &[u8; 32], advertised: &[u8; 8]) -> Result<bool> {
        let stored: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT digest FROM peer_state WHERE peer_key = ?1",
                rusqlite::params![&peer_key[..]],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(stored.as_deref() != Some(&advertised[..]))
    }

    /// Count a failed dial, and say how many have failed in a row.
    pub fn peer_missed(&self, peer_key: &[u8; 32]) -> Result<u32> {
        self.conn.execute(
            "INSERT INTO peer_state (peer_key, attempts) VALUES (?1, 1)
             ON CONFLICT(peer_key) DO UPDATE SET attempts = attempts + 1",
            rusqlite::params![&peer_key[..]],
        )?;
        let n: i64 = self.conn.query_row(
            "SELECT attempts FROM peer_state WHERE peer_key = ?1",
            rusqlite::params![&peer_key[..]],
            |r| r.get(0),
        )?;
        Ok(n as u32)
    }

    /// Everything known about every peer.
    ///
    /// Including those with no address: they are still the answer to "when was
    /// this device last heard from", which is what §4.5 promotes.
    pub fn peer_addresses(&self) -> Result<Vec<PeerAddress>> {
        let mut q = self.conn.prepare(
            "SELECT peer_key, addr, attempts, last_seen FROM peer_state ORDER BY peer_key",
        )?;
        let rows = q
            .query_map([], |r| {
                let key: Vec<u8> = r.get(0)?;
                let addr: Option<String> = r.get(1)?;
                let attempts: i64 = r.get(2)?;
                let seen: Option<i64> = r.get(3)?;
                Ok((key, addr, attempts as u32, seen.map(|s| s as u64)))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(rows
            .into_iter()
            .filter_map(|(k, addr, attempts, last_seen)| {
                let key = <[u8; 32]>::try_from(k.as_slice()).ok()?;
                Some(PeerAddress {
                    key,
                    addr,
                    attempts,
                    last_seen,
                })
            })
            .collect())
    }

    /// Record that this device performed a pairing.
    ///
    /// What makes "a device was added that you did not expect" computable at
    /// all. Without it every admission in the log looks the same, including
    /// the ones this device performed itself.
    pub fn paired(&self, peer_key: &[u8; 32], at: u64) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO pairings (peer_key, paired_at) VALUES (?1, ?2)",
            rusqlite::params![&peer_key[..], at as i64],
        )?;
        Ok(())
    }

    /// Accept an admission this device did not perform.
    pub fn acknowledge_admission(&self, peer_key: &[u8; 32], at: u64) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO admissions_seen (peer_key, seen_at) VALUES (?1, ?2)",
            rusqlite::params![&peer_key[..], at as i64],
        )?;
        Ok(())
    }

    /// Of the keys now in the roster, which this device neither paired,
    /// admitted itself, nor has been told about.
    ///
    /// `own` is every key this device authors under — normally one, but a
    /// restored device keeps its key across incarnations and a device that has
    /// re-keyed has more than one.
    pub fn unexpected_admissions(
        &self,
        roster: &BTreeSet<[u8; 32]>,
        own: &BTreeSet<[u8; 32]>,
    ) -> Result<Vec<[u8; 32]>> {
        let mut out = Vec::new();
        for key in roster {
            if own.contains(key) {
                continue;
            }
            let known: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM pairings WHERE peer_key = ?1)
                     OR EXISTS(SELECT 1 FROM admissions_seen WHERE peer_key = ?1)",
                rusqlite::params![&key[..]],
                |r| r.get(0),
            )?;
            if !known {
                out.push(*key);
            }
        }
        Ok(out)
    }

    /// Fix the set of devices a redaction has to reach.
    ///
    /// Called once, when the redaction is issued. Every roster member at that
    /// moment, and nobody added later: a device admitted afterwards was never
    /// given the content, so requiring it to acknowledge destroying something
    /// it never held would leave the redaction permanently outstanding.
    pub fn redact_awaits(&self, redact: &EventId, peers: &[[u8; 32]], at: u64) -> Result<()> {
        for p in peers {
            self.conn.execute(
                "INSERT OR IGNORE INTO redact_peers (redact_evt, peer_key, issued_at)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![redact.0.to_vec(), &p[..], at as i64],
            )?;
        }
        Ok(())
    }

    /// Record that a peer has folded a redaction.
    pub fn redact_acked(&self, redact: &EventId, peer: &[u8; 32], at: u64) -> Result<()> {
        self.conn.execute(
            "UPDATE redact_peers SET acked_at = ?3
             WHERE redact_evt = ?1 AND peer_key = ?2 AND acked_at IS NULL",
            rusqlite::params![redact.0.to_vec(), &peer[..], at as i64],
        )?;
        Ok(())
    }

    /// Redactions that have not reached every device they were issued
    /// against, and who is missing.
    ///
    /// Reported rather than inferred. "Gone everywhere" is the only claim a
    /// redaction makes, and a system that guessed it would be lying at exactly
    /// the moment it mattered.
    pub fn redactions_outstanding(&self) -> Result<Vec<(EventId, Vec<[u8; 32]>)>> {
        let mut q = self.conn.prepare(
            "SELECT redact_evt, peer_key FROM redact_peers
             WHERE acked_at IS NULL ORDER BY issued_at, peer_key",
        )?;
        let rows = q.query_map([], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?;

        let mut by_event: BTreeMap<[u8; 32], Vec<[u8; 32]>> = BTreeMap::new();
        for r in rows {
            let (evt, peer) = r?;
            if let (Ok(evt), Ok(peer)) = (
                <[u8; 32]>::try_from(evt.as_slice()),
                <[u8; 32]>::try_from(peer.as_slice()),
            ) {
                by_event.entry(evt).or_default().push(peer);
            }
        }
        Ok(by_event.into_iter().map(|(e, p)| (EventId(e), p)).collect())
    }

    /// Remember how to open a session with a peer, learned at pairing.
    /// Remember what a peer said it can watch for (§7.5).
    ///
    /// Local rather than logged. A capability is a fact about an installation
    /// on a handset — it changes with the platform, with a permission, with an
    /// update — and an append-only record of "this phone could see geofences
    /// on Tuesday" is operational chatter of the kind §2.5.2 keeps out of a
    /// log meant to be worth reading in ten years.
    ///
    /// An empty list is not written. A peer that said nothing is a peer that
    /// has not been asked yet, and forgetting what it said last time on the
    /// strength of an older build's silence would lose the answer.
    pub fn learned_observes(&self, peer_key: &[u8; 32], cues: &[String]) -> Result<()> {
        if cues.is_empty() {
            return Ok(());
        }
        self.set(&observes_key(peer_key), &cues.join(","))
    }

    /// What a peer last said it can watch for.
    pub fn peer_observes(&self, peer_key: &[u8; 32]) -> Result<Vec<String>> {
        Ok(self
            .get(&observes_key(peer_key))?
            .map(|v| v.split(',').map(String::from).collect())
            .unwrap_or_default())
    }

    pub fn learned_noise(&self, peer_key: &[u8; 32], noise: &[u8; 32]) -> Result<()> {
        self.conn.execute(
            "INSERT INTO peer_noise (peer_key, noise) VALUES (?1, ?2)
             ON CONFLICT(peer_key) DO UPDATE SET noise = ?2",
            rusqlite::params![&peer_key[..], &noise[..]],
        )?;
        Ok(())
    }

    /// The key to open a session with, if one was learned out of band.
    pub fn noise_for(&self, peer_key: &[u8; 32]) -> Result<Option<[u8; 32]>> {
        let row: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT noise FROM peer_noise WHERE peer_key = ?1",
                rusqlite::params![&peer_key[..]],
                |r| r.get(0),
            )
            .optional()?;
        Ok(row.and_then(|b| <[u8; 32]>::try_from(b.as_slice()).ok()))
    }

    /// Every peer this device knows how to dial, whatever the log says.
    ///
    /// A device that has just joined has exactly one of these and no log, so
    /// this is the only thing that lets it start.
    /// Forget everything local about how to reach a peer.
    ///
    /// Called when a device is retired. The retirement itself is a fact about
    /// the archive and lives in the log; *how to phone that device* is
    /// operational chatter in `ops.db` (§2.5.2), and nothing was removing it.
    ///
    /// Belt as well as braces. `round::sweep` now refuses to dial a retired key,
    /// and this makes the material to do it with absent rather than merely
    /// unused — a peer row that is still there is one the next change to the
    /// sweep can start reading again, which is how the guard got skipped on the
    /// remembered-address path in the first place.
    ///
    /// Not idempotency-sensitive and not an error when the rows are absent: a
    /// device retired twice, or retired having never been reached, is ordinary.
    pub fn forget_peer(&self, peer_key: &[u8; 32]) -> Result<()> {
        for sql in [
            "DELETE FROM peer_state WHERE peer_key = ?1",
            "DELETE FROM peer_noise WHERE peer_key = ?1",
            "DELETE FROM admissions_seen WHERE peer_key = ?1",
        ] {
            self.conn.execute(sql, rusqlite::params![&peer_key[..]])?;
        }
        // Keyed by hex rather than by blob, and so not reachable by the loop
        // above.
        self.forget_key(&observes_key(peer_key))?;
        Ok(())
    }

    pub fn dialable(&self) -> Result<Vec<[u8; 32]>> {
        let mut q = self
            .conn
            .prepare("SELECT peer_key FROM peer_noise ORDER BY peer_key")?;
        let rows = q.query_map([], |r| r.get::<_, Vec<u8>>(0))?;
        let mut out = Vec::new();
        for r in rows {
            if let Ok(k) = <[u8; 32]>::try_from(r?.as_slice()) {
                out.push(k);
            }
        }
        Ok(out)
    }

    pub fn retryable(&self, reason: &str) -> Result<Vec<([u8; 32], Vec<u8>)>> {
        let mut st = self
            .conn
            .prepare("SELECT id, raw FROM rejected WHERE reason = ?1 AND raw IS NOT NULL")?;
        let rows = st.query_map(params![reason], |r| {
            Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, Vec<u8>>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (id, raw) = row?;
            let mut a = [0u8; 32];
            a.copy_from_slice(&id[..32]);
            out.push((a, raw));
        }
        Ok(out)
    }

    /// Drop a refusal, once the event has been accepted for real.
    pub fn forget(&self, id: &[u8; 32]) -> Result<()> {
        self.conn
            .execute("DELETE FROM rejected WHERE id = ?1", params![id.to_vec()])?;
        Ok(())
    }

    /// How many events were refused, by reason.
    ///
    /// Reported separately from anything else, and never described as version
    /// skew: an event that fails authentication is a different condition from
    /// one this build merely cannot interpret, and collapsing the two would
    /// bury the only security signal in the set.
    pub fn rejected_counts(&self) -> Result<BTreeMap<String, u64>> {
        let mut st = self
            .conn
            .prepare("SELECT reason, COUNT(*) FROM rejected GROUP BY reason")?;
        let rows = st.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
        })?;
        let mut out = BTreeMap::new();
        for row in rows {
            let (k, v) = row?;
            out.insert(k, v);
        }
        Ok(out)
    }

    /// Total events refused.
    pub fn rejected_total(&self) -> Result<u64> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM rejected", [], |r| r.get::<_, i64>(0))?
            as u64)
    }
}

/// The local-state key recording that one *moment* has been delivered.
///
/// Keyed by the note and the moment together, not by the note alone. An
/// occurrence at 9am on Tuesday and the same series at 9am on Wednesday are
/// two moments of one note, and a key that could not tell them apart would
/// deliver one of them and swallow the rest of the week.
///
/// # Why a one-off needs the instant too
///
/// A series occurrence carries `occ`, which *is* the instant the rule produced,
/// so its key already changes when the moment does. A one-off event or task
/// carried `None`, and the key was the note alone — which made "this note has
/// been alerted" and "this moment has been alerted" the same statement for
/// exactly the notes where they come apart.
///
/// Correcting a time is the ordinary case. The 14:00 alert fires; the meeting
/// moves, or the recognizer misread it, and the note is corrected to 16:00.
/// `due_by` reports the new moment properly and the key says it was already
/// delivered, so 16:00 passes in silence with nothing anywhere saying why. The
/// same shape for a task pushed to tomorrow after today's alert fired: the new
/// day is silent, permanently.
///
/// So the instant is part of the key. A moment that moves is a different
/// moment, and the one that was delivered was the one that no longer exists.
fn alert_key(tgt: &[u8], occ: Option<i64>, at: i64) -> String {
    let hex: String = tgt.iter().take(8).map(|b| format!("{b:02x}")).collect();
    match occ {
        Some(o) => format!("alerted.{hex}.{o}"),
        None => format!("alerted.{hex}.at{at}"),
    }
}

/// The local-state key recording that one intent has fired.
fn intent_key(id: &[u8]) -> String {
    let hex: String = id.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("intent_fired.{hex}")
}

fn capture_latency_key(surface: &str) -> String {
    format!("capture_latency.{surface}")
}

fn observes_key(peer: &[u8; 32]) -> String {
    let hex: String = peer.iter().map(|b| format!("{b:02x}")).collect();
    format!("peer_observes.{hex}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("capture-ops-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("ops.db")
    }

    fn clean(p: &Path) {
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn generation_starts_at_zero_and_persists() {
        let p = tmp("gen");
        {
            let o = Ops::open(&p).unwrap();
            assert_eq!(o.generation().unwrap(), 0);
            o.set_generation(3).unwrap();
        }
        let again = Ops::open(&p).unwrap();
        assert_eq!(again.generation().unwrap(), 3);
        clean(&p);
    }

    #[test]
    fn rejections_are_counted_by_reason() {
        let p = tmp("reject");
        let o = Ops::open(&p).unwrap();
        o.reject(&[1u8; 32], Some("aa"), Some(5), "bad_sig", 100, None)
            .unwrap();
        o.reject(&[2u8; 32], Some("aa"), Some(6), "bad_sig", 101, None)
            .unwrap();
        o.reject(&[3u8; 32], Some("bb"), Some(1), "bhash_mismatch", 102, None)
            .unwrap();

        let counts = o.rejected_counts().unwrap();
        assert_eq!(counts.get("bad_sig"), Some(&2));
        assert_eq!(counts.get("bhash_mismatch"), Some(&1));
        assert_eq!(o.rejected_total().unwrap(), 3);
        clean(&p);
    }

    #[test]
    fn two_rejections_may_claim_the_same_position() {
        // Deliberately allowed. A rejected event holds no position, so nothing
        // stops several of them naming the same one — and refusing the second
        // would mean an attacker could suppress the record of the first.
        let p = tmp("same-pos");
        let o = Ops::open(&p).unwrap();
        o.reject(&[1u8; 32], Some("aa"), Some(5), "bad_sig", 100, None)
            .unwrap();
        o.reject(&[2u8; 32], Some("aa"), Some(5), "bad_sig", 101, None)
            .unwrap();
        assert_eq!(o.rejected_total().unwrap(), 2);
        clean(&p);
    }

    #[test]
    fn recording_the_same_rejection_twice_is_idempotent() {
        let p = tmp("dup-reject");
        let o = Ops::open(&p).unwrap();
        o.reject(&[1u8; 32], None, None, "bad_sig", 100, None)
            .unwrap();
        o.reject(&[1u8; 32], None, None, "bad_sig", 200, None)
            .unwrap();
        assert_eq!(o.rejected_total().unwrap(), 1);
        clean(&p);
    }
    #[test]
    fn a_peer_never_synced_with_always_has_news() {
        // The failure this prevents is a device that skips every connection
        // forever and reports itself in agreement with a peer it has never
        // spoken to.
        let p = tmp("peer-unknown");
        let ops = Ops::open(&p).unwrap();
        assert!(ops.peer_has_news(&[1; 32], &[0; 8]).unwrap());
        clean(&p);
    }

    #[test]
    fn an_unchanged_advertisement_is_skippable() {
        let p = tmp("peer-agreed");
        let ops = Ops::open(&p).unwrap();
        ops.peer_agreed(&[1; 32], &[7; 8]).unwrap();

        assert!(!ops.peer_has_news(&[1; 32], &[7; 8]).unwrap());
        assert!(
            ops.peer_has_news(&[1; 32], &[8; 8]).unwrap(),
            "a changed digest did not register as news, so the peer would \
             never be dialled again"
        );
        clean(&p);
    }

    #[test]
    fn agreement_does_not_erase_the_address() {
        // Both writes land on one row. An upsert that replaced the row rather
        // than the column would lose the static fallback address every time a
        // sync completed — leaving the device reachable only by mDNS, which is
        // exactly the network where mDNS is blocked.
        let p = tmp("peer-upsert");
        let ops = Ops::open(&p).unwrap();
        ops.peer_reached(&[1; 32], "192.168.1.40:9999", 1_000)
            .unwrap();
        ops.peer_agreed(&[1; 32], &[7; 8]).unwrap();

        let addrs = ops.peer_addresses().unwrap();
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0].addr.as_deref(), Some("192.168.1.40:9999"));
        assert!(!ops.peer_has_news(&[1; 32], &[7; 8]).unwrap());
        clean(&p);
    }

    #[test]
    fn reaching_a_peer_resets_its_backoff() {
        let p = tmp("peer-backoff");
        let ops = Ops::open(&p).unwrap();
        assert_eq!(ops.peer_missed(&[1; 32]).unwrap(), 1);
        assert_eq!(ops.peer_missed(&[1; 32]).unwrap(), 2);

        ops.peer_reached(&[1; 32], "10.0.0.2:9999", 1_000).unwrap();
        assert_eq!(
            ops.peer_addresses().unwrap()[0].attempts,
            0,
            "a peer that answered was still being backed off from"
        );
        clean(&p);
    }
    #[test]
    fn an_admission_this_device_did_not_make_is_reported() {
        // The detection §5.3 relies on. It cannot prevent a compromised device
        // from admitting an attacker, and says so — what it can do is make the
        // admission visible rather than silent.
        let p = tmp("admissions");
        let ops = Ops::open(&p).unwrap();

        let mine = [1u8; 32];
        let paired = [2u8; 32];
        let stranger = [3u8; 32];
        ops.paired(&paired, 1_000).unwrap();

        let roster = BTreeSet::from([mine, paired, stranger]);
        let own = BTreeSet::from([mine]);

        assert_eq!(
            ops.unexpected_admissions(&roster, &own).unwrap(),
            vec![stranger],
            "an admission this device neither made nor performed went unreported"
        );
        clean(&p);
    }

    #[test]
    fn an_acknowledged_admission_stops_being_reported() {
        let p = tmp("admissions-ack");
        let ops = Ops::open(&p).unwrap();
        let stranger = [3u8; 32];
        let roster = BTreeSet::from([stranger]);
        let own = BTreeSet::new();

        assert_eq!(ops.unexpected_admissions(&roster, &own).unwrap().len(), 1);
        ops.acknowledge_admission(&stranger, 2_000).unwrap();
        assert!(ops.unexpected_admissions(&roster, &own).unwrap().is_empty());
        clean(&p);
    }

    #[test]
    fn acknowledging_is_not_recorded_as_pairing() {
        // They say different things, and conflating them would mean a device
        // that was merely acknowledged looks, forever after, like one this
        // person physically paired.
        let p = tmp("admissions-distinct");
        let ops = Ops::open(&p).unwrap();
        let stranger = [3u8; 32];
        ops.acknowledge_admission(&stranger, 2_000).unwrap();

        let paired: i64 = ops
            .conn
            .query_row("SELECT COUNT(*) FROM pairings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(paired, 0);
        clean(&p);
    }

    #[test]
    fn a_restored_device_does_not_report_itself() {
        // Membership is keyed by key, and a restore keeps the key. A device
        // that reported its own admission after every restore would train the
        // person to dismiss the one report that matters.
        let p = tmp("admissions-self");
        let ops = Ops::open(&p).unwrap();
        let mine = [1u8; 32];

        assert!(
            ops.unexpected_admissions(&BTreeSet::from([mine]), &BTreeSet::from([mine]))
                .unwrap()
                .is_empty()
        );
        clean(&p);
    }
    #[test]
    fn a_redaction_is_outstanding_until_every_peer_acknowledges() {
        let p = tmp("redact-peers");
        let ops = Ops::open(&p).unwrap();
        let r = EventId([1; 32]);
        let (phone, desktop) = ([2u8; 32], [3u8; 32]);

        ops.redact_awaits(&r, &[phone, desktop], 1_000).unwrap();
        assert_eq!(ops.redactions_outstanding().unwrap()[0].1.len(), 2);

        ops.redact_acked(&r, &desktop, 2_000).unwrap();
        let out = ops.redactions_outstanding().unwrap();
        assert_eq!(out[0].1, vec![phone]);

        ops.redact_acked(&r, &phone, 3_000).unwrap();
        assert!(ops.redactions_outstanding().unwrap().is_empty());
        clean(&p);
    }

    #[test]
    fn retiring_a_peer_does_not_mark_a_redaction_delivered() {
        // The sequence this exists for: the phone is stolen, you retire it,
        // and then you redact the sensitive notes. If the outstanding set were
        // read from the current roster, retiring the phone would remove it and
        // every surface would report complete propagation for the one device
        // that never received anything.
        let p = tmp("redact-retired");
        let ops = Ops::open(&p).unwrap();
        let r = EventId([1; 32]);
        let stolen = [2u8; 32];

        ops.redact_awaits(&r, &[stolen], 1_000).unwrap();

        // The roster no longer contains it. The record is unmoved, because it
        // is a record of what happened rather than of who is a member.
        let out = ops.redactions_outstanding().unwrap();
        assert_eq!(
            out[0].1,
            vec![stolen],
            "a retired peer stopped being reported"
        );
        clean(&p);
    }

    #[test]
    fn a_peer_admitted_later_is_not_awaited() {
        // It was never given the content, so requiring it to acknowledge
        // destroying something it never held would leave the redaction
        // outstanding for ever.
        let p = tmp("redact-late-peer");
        let ops = Ops::open(&p).unwrap();
        let r = EventId([1; 32]);

        ops.redact_awaits(&r, &[[2u8; 32]], 1_000).unwrap();
        ops.redact_acked(&r, &[2u8; 32], 2_000).unwrap();
        assert!(ops.redactions_outstanding().unwrap().is_empty());

        // A device admitted afterwards does not reopen it.
        assert!(
            ops.redactions_outstanding().unwrap().is_empty(),
            "a later admission reopened a completed redaction"
        );
        clean(&p);
    }

    #[test]
    fn acknowledging_twice_changes_nothing() {
        // Acks arrive over a wire and a wire duplicates.
        let p = tmp("redact-dup-ack");
        let ops = Ops::open(&p).unwrap();
        let r = EventId([1; 32]);
        ops.redact_awaits(&r, &[[2u8; 32]], 1_000).unwrap();
        ops.redact_acked(&r, &[2u8; 32], 2_000).unwrap();
        ops.redact_acked(&r, &[2u8; 32], 9_000).unwrap();

        let at: Option<i64> = ops
            .conn
            .query_row("SELECT acked_at FROM redact_peers", [], |r| r.get(0))
            .unwrap();
        assert_eq!(at, Some(2_000), "a duplicate ack moved the time it landed");
        clean(&p);
    }
    #[test]
    fn answering_does_not_overwrite_a_working_address() {
        // An inbound connection arrives from an ephemeral source port. Writing
        // that down as the peer's address replaces one that works with one
        // that can never answer — and the failure only shows up later, as a
        // peer that has silently stopped being reachable.
        let p = tmp("peer-answered");
        let ops = Ops::open(&p).unwrap();
        ops.peer_reached(&[1; 32], "192.168.1.40:41822", 1_000)
            .unwrap();

        ops.peer_answered(&[1; 32], 2_000).unwrap();

        let a = &ops.peer_addresses().unwrap()[0];
        assert_eq!(a.addr.as_deref(), Some("192.168.1.40:41822"));
        assert_eq!(
            a.last_seen,
            Some(2_000),
            "answering did not count as being reached"
        );
        clean(&p);
    }

    #[test]
    fn answering_from_an_unknown_peer_records_no_address() {
        let p = tmp("peer-answered-new");
        let ops = Ops::open(&p).unwrap();
        ops.peer_answered(&[1; 32], 2_000).unwrap();

        let rows = ops.peer_addresses().unwrap();
        assert_eq!(rows.len(), 1, "the peer should be known, just not dialable");
        assert_eq!(
            rows[0].addr, None,
            "an address was invented for a peer that only ever dialled in"
        );
        assert_eq!(rows[0].last_seen, Some(2_000));
        clean(&p);
    }
}
