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
    /// The day this note belongs to, as `2026-09-15`.
    ///
    /// Computed in the note's *own* zone, not the reader's. A note captured at
    /// 23:40 in one place must not move to a different day when read in
    /// another — the timeline groups by where you were, and travel is not a
    /// reason for yesterday to become today (§2.3).
    ///
    /// Supplied rather than derived by the caller because that is a decision
    /// about what a day is, and there is one implementation of it.
    pub day: String,
    /// The capture time, rendered in the note's own zone.
    pub at: String,
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

/// One thing happening on a day: an occurrence of a series, or a task due.
///
/// A list rather than a column-packed grid. The desktop packs overlapping
/// entries into columns because it has the width for it; a phone agenda is
/// read top to bottom, and packing would be work done to produce a layout
/// nothing renders.
/// A destruction ordered and not yet carried out.
///
/// Carries when the window closes rather than how long is left: a duration
/// computed once and rendered on a screen that stays up is wrong within a
/// minute, and wrong in the direction of appearing to have more time.
#[derive(Debug, uniffi::Record)]
pub struct Pending {
    /// The id that cancels it.
    pub id: String,
    pub day: String,
    pub at: String,
}

#[derive(Debug, uniffi::Record)]
pub struct AgendaEntry {
    pub slug: String,
    pub text: Option<String>,
    /// When it starts. For an all-day entry this is the start of the day in
    /// the archive's terms, not a real instant.
    pub start_ms: i64,
    /// A day rather than a moment. §3.6: "due Friday" is deliberately a
    /// floating date, because 00:00 UTC on Friday is a different claim.
    pub all_day: bool,
    /// A task that is due, rather than an event that occupies time.
    pub due: bool,
    /// Already ticked off.
    pub done: bool,
}

/// A named page, as the wiki sidebar lists it.
#[derive(Debug, uniffi::Record)]
pub struct Named {
    pub slug: String,
    pub alias: Option<String>,
    /// The page this one sits under, where it sits under one.
    pub parent: Option<String>,
}

/// Something true about the archive that is worth saying.
#[derive(Debug, uniffi::Record)]
pub struct Condition {
    /// `attention` or `worth-knowing`. Two tiers, because a list where
    /// everything is urgent is one where nothing is.
    pub tier: String,
    pub text: String,
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
        let _writing = self.write_lock()?;

        match capture_store::session::capture(&self.paths, &text)? {
            Committed::Stored(slug) => Ok(Captured::Stored { slug }),
            Committed::Spooled => Ok(Captured::Spooled),
        }
    }

    /// Capture something shared out of another application.
    ///
    /// The URL is kept in the ambient envelope rather than pasted into the
    /// text: a quotation without its source is a thing you cannot go back to,
    /// and putting it in the text would make it part of what you wrote — it
    /// would be searched, edited and redacted as if you had typed it.
    ///
    /// §2.3's envelope has carried `source_url` since M1 with nothing able to
    /// fill it, because the desktop has no share gesture. This is the first
    /// surface that does.
    pub fn capture_shared(&self, text: String, source_url: Option<String>) -> Result<Captured> {
        let _writing = self.write_lock()?;
        let body = capture_store::session::capture_body_from(&text, source_url.as_deref())?;

        match capture_store::session::commit_body(&self.paths, &body)? {
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

    /// Today, in this device's own zone.
    ///
    /// Asked for rather than computed by the caller, for the same reason the
    /// agenda's date is parsed here: what day it is depends on a zone, and
    /// there is one implementation of that.
    pub fn today(&self) -> String {
        let now = jiff::Timestamp::now();
        now.to_zoned(jiff::tz::TimeZone::system())
            .strftime("%Y-%m-%d")
            .to_string()
    }

    /// What is happening on a day, and what is due.
    ///
    /// The date is a string like `2026-09-15` and is interpreted here rather
    /// than by the caller. §3.6 and the time model decide what a day is,
    /// including the daylight-saving cases, and a second answer to that
    /// question is a calendar that disagrees with itself.
    pub fn agenda(&self, day: String) -> Result<Vec<AgendaEntry>> {
        let store = self.read()?;
        let zone = capture_time::Zone::new("UTC").expect("UTC is always present");
        let date =
            capture_time::FloatingDate::parse(&day).ok_or_else(|| CaptureError::Refused {
                reason: format!("{day:?} is not a date — write it as 2026-09-15"),
            })?;
        let from =
            capture_time::at_local(date, 0, 0, zone).ok_or_else(|| CaptureError::Refused {
                reason: format!("{day:?} is a day that does not exist"),
            })?;
        let to = from + 24 * 60 * 60 * 1000;

        let mut out = Vec::new();

        // All-day entries first, in their own band. They are floating dates
        // with no position on an hour axis (§4.4).
        for tgt in store.all_day_on(&day)? {
            out.push(AgendaEntry {
                slug: store.slug_of(&tgt)?.unwrap_or_default(),
                text: store.text_of(&tgt)?,
                start_ms: from,
                all_day: true,
                due: false,
                done: false,
            });
        }

        for (tgt, occ) in store.occurring_between(from, to)? {
            out.push(AgendaEntry {
                slug: store.slug_of(&tgt)?.unwrap_or_default(),
                text: store.text_of(&tgt)?,
                start_ms: occ.start_ms,
                all_day: occ.all_day,
                due: false,
                done: occ.done,
            });
        }

        for (tgt, done) in store.tasks_due_on(&day)? {
            out.push(AgendaEntry {
                slug: store.slug_of(&tgt)?.unwrap_or_default(),
                text: store.text_of(&tgt)?,
                start_ms: from,
                all_day: true,
                due: true,
                done,
            });
        }

        out.sort_by_key(|e| (e.all_day, e.start_ms));
        Ok(out)
    }

    /// Every page that has a name, for the wiki.
    pub fn named(&self) -> Result<Vec<Named>> {
        let store = self.read()?;
        Ok(store
            .named()?
            .into_iter()
            .map(|n| Named {
                slug: n.slug,
                alias: Some(n.alias),
                parent: n.parent,
            })
            .collect())
    }

    /// What the archive would like someone to know.
    ///
    /// Two tiers, and they are not severities: `attention` is something
    /// wanting a decision, `worth-knowing` is something true that needs
    /// nothing. A list where everything is urgent is one where nothing is.
    pub fn health(&self) -> Result<Vec<Condition>> {
        let store = self.read()?;
        let ops = capture_store::ops::Ops::open_read(&self.paths.ops)?;
        let mut out = Vec::new();

        // Authentication failures are never folded into a general total. An
        // event that failed to authenticate is a security signal, and burying
        // it beside version skew is how it goes unnoticed.
        for (reason, n) in ops.rejected_counts()? {
            out.push(Condition {
                tier: "attention".into(),
                text: format!("{n} event(s) failed authentication ({reason})"),
            });
        }

        if capture_store::spool::in_use(&self.paths.spool)? {
            out.push(Condition {
                tier: "attention".into(),
                text: "captures are being spooled — the store could not be written".into(),
            });
        }

        for (_id, owed) in ops.redactions_outstanding()? {
            out.push(Condition {
                tier: "attention".into(),
                text: format!(
                    "a redaction has not been acknowledged by {} device(s); until it is, \
                     the content may still exist on them",
                    owed.len()
                ),
            });
        }

        for (_id, reason) in store.quarantined()? {
            out.push(Condition {
                tier: "worth-knowing".into(),
                text: format!("an event was set aside: {reason}"),
            });
        }

        Ok(out)
    }

    /// How many notes the archive holds.
    pub fn note_count(&self) -> Result<u64> {
        Ok(self.read()?.note_count()?)
    }

    // ---------------------------------------------------------- writing ----
    //
    // Every one of these takes the writer lock, does one thing, and releases
    // it. None of them decides anything: what a mute covers, which edit wins,
    // when a redaction executes — all of that is the fold's, and is identical
    // to what the desktop does because it is the same code.

    /// Replace a note's text. The old version stays in the log.
    pub fn edit(&self, name: String, text: String) -> Result<()> {
        let _w = self.write_lock()?;
        Ok(capture_store::mutate::edit(&self.paths, &name, &text)?)
    }

    pub fn tag(&self, name: String, tag: String) -> Result<()> {
        let _w = self.write_lock()?;
        Ok(capture_store::mutate::tag(&self.paths, &name, &tag)?)
    }

    pub fn untag(&self, name: String, tag: String) -> Result<()> {
        let _w = self.write_lock()?;
        Ok(capture_store::mutate::untag(&self.paths, &name, &tag)?)
    }

    /// Give a note a chosen name, or clear it with `None`.
    pub fn set_alias(&self, name: String, alias: Option<String>) -> Result<()> {
        let _w = self.write_lock()?;
        Ok(capture_store::mutate::set_alias(
            &self.paths,
            &name,
            alias.as_deref(),
        )?)
    }

    pub fn pin(&self, name: String, pinned: bool) -> Result<()> {
        let _w = self.write_lock()?;
        Ok(capture_store::mutate::pin(&self.paths, &name, pinned)?)
    }

    /// Suppress a note. It stays in the timeline and leaves search.
    ///
    /// §2.6 exists so that suppressing something is cheaper than destroying
    /// it. A phone that could destroy but not mute would invert exactly the
    /// gradient that argument creates.
    pub fn mute(&self, name: String) -> Result<()> {
        let _w = self.write_lock()?;
        Ok(capture_store::mutate::mute_note(&self.paths, &name)?)
    }

    /// Put a page under another, or back at the top with `None`.
    pub fn set_parent(&self, name: String, under: Option<String>) -> Result<()> {
        let _w = self.write_lock()?;
        Ok(capture_store::mutate::set_parent(
            &self.paths,
            &name,
            under.as_deref(),
        )?)
    }

    /// Order a note's destruction. Returns the id that cancels it.
    ///
    /// Queued rather than immediate: §1.1's window is what makes the gesture
    /// recoverable, and it is the same window here as everywhere else.
    pub fn redact(&self, name: String) -> Result<String> {
        let _w = self.write_lock()?;
        Ok(capture_store::mutate::redact(&self.paths, &name)?)
    }

    /// The destruction waiting on a note, where one is waiting.
    ///
    /// Exists because `cancel_redaction` is keyed by the id `redact` returned,
    /// and a phone does not survive long enough to keep it. The application is
    /// destroyed and rebuilt constantly — on rotation, on a low-memory kill,
    /// on being backgrounded for a minute — and a window that can only be
    /// cancelled by the process that opened it is not the window §1.1
    /// describes. It is one that closes when the platform feels like it.
    ///
    /// So the id is asked for again rather than held. The queue is in the
    /// projection and outlives every process that reads it.
    pub fn pending_redaction(&self, name: String) -> Result<Option<Pending>> {
        let store = self.read()?;
        let tgt = store
            .resolve(&name)?
            .ok_or_else(|| CaptureError::NotFound { name: name.clone() })?;

        let waiting = store
            .redact_queue()?
            .into_iter()
            .find(|(_, on, _, cancelled)| *on == tgt && !*cancelled);

        Ok(waiting.map(|(redact, _, execute_after, _)| {
            // This device's zone, not the note's. A capture time belongs to
            // where you were; a deadline is happening in front of you now.
            let here = jiff::tz::TimeZone::system();
            let (day, at) = rendered(execute_after as i64, here.iana_name());
            Pending {
                id: capture_store::mutate::short(&redact),
                day,
                at,
            }
        }))
    }

    /// Stop a queued destruction, by the id `redact` returned.
    pub fn cancel_redaction(&self, id_prefix: String) -> Result<()> {
        let _w = self.write_lock()?;
        Ok(capture_store::mutate::cancel_redaction(
            &self.paths,
            &id_prefix,
        )?)
    }

    /// Mark a task done, or not.
    pub fn set_done(&self, name: String, done: bool) -> Result<()> {
        let _w = self.write_lock()?;
        Ok(capture_store::promote::set_done(&self.paths, &name, done)?)
    }

    /// Carry out whatever has come due — redactions past their window,
    /// promotions, expiries.
    ///
    /// Called when the application comes to the foreground and on the sync
    /// tick. On the desktop this happens on every command that writes; a phone
    /// has no equivalent, so it is asked for explicitly.
    pub fn settle(&self) -> Result<u64> {
        let _w = self.write_lock()?;
        let mut s = capture_store::session::Session::open(self.paths.clone())?;
        let done = s.drain_due()?;
        Ok((done.destroyed + done.expired + done.resumed) as u64)
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
    /// Serialise writes from this process.
    fn write_lock(&self) -> Result<std::sync::MutexGuard<'_, ()>> {
        self.writing.lock().map_err(|_| CaptureError::Store {
            reason: "a previous write left the archive locked".into(),
        })
    }

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

/// A moment, in the zone it happened in.
///
/// Both halves are here rather than on the calling side because both depend on
/// which zone the note belongs to, and that is a rule rather than a
/// presentation choice. What the caller does with the strings is its business;
/// what day a note is on is not.
fn rendered(ms: i64, tz: Option<&str>) -> (String, String) {
    let zone = tz
        .and_then(|z| jiff::tz::TimeZone::get(z).ok())
        .unwrap_or(jiff::tz::TimeZone::UTC);

    match jiff::Timestamp::from_millisecond(ms) {
        Ok(t) => {
            let z = t.to_zoned(zone);
            (
                z.strftime("%Y-%m-%d").to_string(),
                z.strftime("%H:%M").to_string(),
            )
        }
        // A timestamp outside what the calendar can express. Not worth
        // refusing a read over — the note exists and its text is what matters.
        Err(_) => ("unknown".into(), String::new()),
    }
}

fn summarise(n: capture_store::projection::NoteRow) -> NoteSummary {
    let (day, at) = rendered(n.created.0 as i64, n.tz.as_deref());
    NoteSummary {
        slug: n.slug,
        alias: n.alias,
        text: n.text,
        captured_ms: n.created.0 as i64,
        day,
        at,
        pinned: n.pinned,
        muted_until_ms: n.muted_until.map(|t| t as i64),
        redacted: n.redacted_at.is_some(),
        redaction_queued: n.redaction_queued,
    }
}
