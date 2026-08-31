//! What the phone may ask for.
//!
//! # The whole boundary is here
//!
//! Everything Kotlin can do is a method in this file. There is no query
//! interface and no way to reach the store except through one of these, which
//! is the point of writing them out: the surface is enumerable, and adding to
//! it is a deliberate act with a diff.
//!
//! # Operation-shaped, not type-shaped
//!
//! These are operations — capture, read, search — rather than the store's
//! types. A fine-grained surface invites the caller to assemble behaviour out
//! of primitives, and the first thing assembled that way will be something the
//! fold already decides. The conflict rules in §6 were property-tested across
//! two milestones on the assumption that there is exactly one implementation
//! of them; a second one in Kotlin would not be a duplicate, it would be the
//! one nobody exercises, diverging silently because both sides look correct in
//! isolation.
//!
//! So: **Kotlin renders and handles lifecycle. It does not decide.** No date
//! arithmetic, no conflict resolution, no slug generation, no recurrence
//! expansion. There is a mechanical check for this in the Android build.
//!
//! # Where the store lives is passed in, never resolved
//!
//! The desktop finds its store from `$CAPTURE_HOME` or a home directory.
//! Neither exists in a meaningful form on Android, where the application is
//! told its own private directory at runtime and it differs by user profile
//! and install. Passing it in is not a concession — resolving it here would
//! mean guessing, and guessing wrong means a second empty archive that looks
//! exactly like data loss.

use std::sync::Mutex;

use capture_store::paths::StorePaths;
use capture_store::session::Committed;

uniffi::setup_scaffolding!();

/// What went wrong, in terms the phone can act on.
///
/// Deliberately few variants. A caller that cannot tell "the disk is full"
/// from "that note does not exist" will treat both as a crash; a caller given
/// twenty variants will match on the two it recognises and treat the rest as a
/// crash anyway. These are the distinctions that change what a person should
/// do next.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CaptureError {
    /// The archive is not there, or not usable.
    #[error("the archive could not be opened: {reason}")]
    Store { reason: String },

    /// Asked for something that is not there.
    #[error("no note named {name}")]
    NotFound { name: String },

    /// The request itself does not make sense.
    #[error("{reason}")]
    Refused { reason: String },
}

impl From<capture_store::Error> for CaptureError {
    fn from(e: capture_store::Error) -> Self {
        match e {
            capture_store::Error::NotFound(name) => CaptureError::NotFound { name },
            capture_store::Error::Usage(reason) => CaptureError::Refused { reason },
            other => CaptureError::Store {
                reason: other.to_string(),
            },
        }
    }
}

type Result<T> = std::result::Result<T, CaptureError>;

/// One note, as a list shows it.
#[derive(Debug, uniffi::Record)]
pub struct NoteSummary {
    /// The permanent name. Stable for the life of the note.
    pub slug: String,
    /// A chosen name, where one was given. Shares one namespace with slugs.
    pub alias: Option<String>,
    /// Absent when the note is redacted, has a redaction queued, or has no
    /// readable text yet. Absence is meaningful and is not an empty string.
    pub text: Option<String>,
    /// Milliseconds since the epoch.
    pub captured_ms: i64,
    pub pinned: bool,
    /// Muted until this moment, where it is muted at all.
    pub muted_until_ms: Option<i64>,
    /// Destroyed. The content is gone and is not recoverable.
    pub redacted: bool,
    /// A destruction has been ordered and not yet carried out.
    ///
    /// Kept separate from `redacted` deliberately. Between the two the content
    /// is withheld but still there, and that is the state a person can still
    /// cancel — collapsing them into one flag would hide the only one of the
    /// two they can do anything about.
    pub redaction_queued: bool,
}

/// Whether a capture reached the log or the spool.
///
/// Both mean "it is safe". They differ in whether it can be found yet, and a
/// surface that reported them identically would be claiming something it does
/// not know.
#[derive(Debug, uniffi::Enum)]
pub enum Captured {
    /// In the log, under this name.
    Stored { slug: String },
    /// In the spool, because the store would not take it. Safe, not yet
    /// findable, committed on the next successful write.
    Spooled,
}

/// An open archive.
///
/// Holds no connection between calls. Every operation opens what it needs and
/// closes it, which costs a little and buys the thing that matters on a phone:
/// the process can be killed between any two calls without leaving a lock
/// behind, and Android kills processes routinely and without warning.
#[derive(uniffi::Object)]
pub struct Archive {
    paths: StorePaths,
    /// Serialises writes from this process.
    ///
    /// The store's own writer lock is what makes concurrent writers safe
    /// *between* processes. This is narrower: several Kotlin coroutines can
    /// call in at once, and two of them racing for the same lock would produce
    /// a spurious "somebody else is writing" for a caller that is the only
    /// process on the device.
    writing: Mutex<()>,
}

#[uniffi::export]
impl Archive {
    /// Open the archive under a directory, creating it on first use.
    ///
    /// `root` is the application's private directory. See the module comment
    /// for why it is passed rather than resolved.
    #[uniffi::constructor]
    pub fn open(root: String) -> Result<Self> {
        let paths = StorePaths::under(&root);

        if !capture_store::store::exists(&paths) {
            let now = jiff::Timestamp::now().as_millisecond().max(0) as u64;
            let fresh = capture_store::store::create(&paths, now)?;

            // The root and the first roster key, in that order. Nothing else
            // can be written until they are in the log: they say which key
            // this origin authors under, and what the mesh advertises under.
            let mut log = capture_store::log::Log::open(&paths.events)?;
            log.append(&fresh.root)?;
            log.append(&fresh.roster_key)?;

            let ops = capture_store::ops::Ops::open(&paths.ops)?;
            let generation = ops.generation()?;
            capture_store::projection::Store::open(&capture_store::projection::generation_path(
                &paths.state,
                generation,
            ))?;

            let (wall, counter) = fresh.clock.persist();
            let mut identity = fresh.identity;
            identity.advance(&paths, fresh.roster_key.header.seq, wall, counter)?;
        } else {
            capture_store::store::open(&paths)?;
        }

        Ok(Archive {
            paths,
            writing: Mutex::new(()),
        })
    }

    /// Capture text, exactly as the desktop's chord does.
    ///
    /// Committed before this returns. It does not queue on the caller's side:
    /// a Quick Settings tile can be destroyed the moment its callback returns,
    /// and §2.1's promise is that a capture the interface accepted is never
    /// lost.
    pub fn capture(&self, text: String) -> Result<Captured> {
        let _writing = self.writing.lock().map_err(|_| CaptureError::Store {
            reason: "a previous write left the archive locked".into(),
        })?;

        match capture_store::session::capture(&self.paths, &text)? {
            Committed::Stored(slug) => Ok(Captured::Stored { slug }),
            Committed::Spooled => Ok(Captured::Spooled),
        }
    }

    /// The most recent notes, newest first.
    pub fn timeline(&self, limit: u32) -> Result<Vec<NoteSummary>> {
        let store = self.read()?;
        Ok(store
            .timeline(limit as usize)?
            .into_iter()
            .map(summarise)
            .collect())
    }

    /// One note by slug or alias.
    pub fn note(&self, name: String) -> Result<NoteSummary> {
        let store = self.read()?;
        let tgt = store
            .resolve(&name)?
            .ok_or_else(|| CaptureError::NotFound { name: name.clone() })?;
        store
            .note(&tgt)?
            .map(summarise)
            .ok_or(CaptureError::NotFound { name })
    }

    /// Full-text search.
    ///
    /// The clock is passed to the store because ranking is evaluated against a
    /// moment rather than against "now" wherever the query happens to run —
    /// the same value the desktop passes.
    pub fn search(&self, query: String, limit: u32) -> Result<Vec<NoteSummary>> {
        let store = self.read()?;
        let now = jiff::Timestamp::now().as_millisecond().max(0) as u64;

        let mut out = Vec::new();
        for tgt in store.search(&query, now)? {
            if out.len() >= limit as usize {
                break;
            }
            if let Some(n) = store.note(&tgt)? {
                out.push(summarise(n));
            }
        }
        Ok(out)
    }

    /// How many notes the archive holds.
    pub fn note_count(&self) -> Result<u64> {
        Ok(self.read()?.note_count()?)
    }

    /// This device's own key, as the surfaces show it.
    ///
    /// Six bytes rather than thirty-two: a prefix nobody reads is worse than
    /// one short enough that they do, and this is what a person compares
    /// against another device's screen when pairing.
    pub fn device_key(&self) -> Result<String> {
        let identity = capture_store::Identity::load(&self.paths)?;
        Ok(identity.public().to_bytes()[..6]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }
}

impl Archive {
    /// The projection, without taking the writer lock.
    ///
    /// Reads never block on a write and never block a write. `capture verify`
    /// exists for the moment the archive is unhealthy, and a diagnostic that
    /// stops working when the system is unwell is one you cannot use when you
    /// need it.
    fn read(&self) -> Result<capture_store::projection::Store> {
        let generation = capture_store::ops::Ops::open_read(&self.paths.ops)?.generation()?;
        Ok(capture_store::projection::Store::open_read(
            &capture_store::projection::generation_path(&self.paths.state, generation),
        )?)
    }
}

fn summarise(n: capture_store::projection::NoteRow) -> NoteSummary {
    NoteSummary {
        slug: n.slug,
        alias: n.alias,
        text: n.text,
        captured_ms: n.created.0 as i64,
        pinned: n.pinned,
        muted_until_ms: n.muted_until.map(|t| t as i64),
        redacted: n.redacted_at.is_some(),
        redaction_queued: n.redaction_queued,
    }
}
