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

/// A device this archive knows about.
#[derive(Debug, uniffi::Record)]
pub struct Peer {
    /// The first bytes of its key, which is how every surface names a device.
    pub key: String,
    /// A remembered address, where one is known. Absent is not a fault: a peer
    /// that only ever dials in is demonstrably alive and has no address here,
    /// because the port it called from was ephemeral.
    pub addr: Option<String>,
    /// When it was last reached, rendered. Never as "fresh" or "stale" —
    /// §6.6, and it matters most on the device that sleeps most: a phone that
    /// implied currency after four hours in a pocket would be lying in the
    /// direction people act on.
    pub last_seen: Option<String>,
    /// Consecutive failed dials.
    pub missed: u32,
    /// What it last said it can watch for (§7.5).
    ///
    /// Empty where it has not said — which includes every device not yet
    /// synced with, and every build that predates the field. That is not the
    /// same as "watches nothing", and a surface offering a notifier should say
    /// "not known yet" rather than imply the device is useless.
    pub observes: Vec<String>,
}

/// What one sync round managed.
#[derive(Debug, uniffi::Record)]
pub struct SyncReport {
    /// Whether a responder could be opened.
    ///
    /// False on Android means the multicast lock was not held, and that is the
    /// trap worth naming: multicast receive then yields no packets and no
    /// error, so discovery looks like it ran against an empty network.
    pub multicast: bool,
    /// Peers that dialled this device during the window.
    pub answered: u32,
    /// The last thing that went wrong, if anything did.
    pub trouble: Option<String>,
}

/// A review session: what is offered, and what keeps slipping.
#[derive(Debug, uniffi::Record)]
pub struct Review {
    pub items: Vec<Resurfaced>,
    /// Notes that have lapsed repeatedly, by name.
    ///
    /// Named rather than counted, because "three things keep slipping" is not
    /// something a person can act on and their names are.
    pub nagging: Vec<String>,
}

/// One note offered for review.
#[derive(Debug, uniffi::Record)]
pub struct Resurfaced {
    pub slug: String,
    pub text: Option<String>,
    /// Why this one: `forward`, `never-engaged` or `context`.
    ///
    /// Said out loud on every card. A resurfacing whose reason is invisible is
    /// indistinguishable from a random one, and §2.7 removed the grade
    /// precisely so that the schedule is not something to be second-guessed.
    pub because: String,
    /// What the archive thinks this might be, where it was unsure enough not
    /// to have acted.
    pub suggestion: Option<String>,
}

/// How much of the time this device may see where it is.
///
/// Three states rather than a flag, because the middle one is a trap. A phone
/// that may see its position *only while the application is open* will accept
/// a geofence, register it, report it as working, and never fire it — because
/// nobody is looking at a notes app while walking into a hardware shop. That
/// is §7.3's failure exactly, and a boolean cannot express it.
#[derive(Debug, PartialEq, uniffi::Enum)]
pub enum Watching {
    /// Not at all.
    Never,
    /// Only while somebody is looking at the application, which for a geofence
    /// means never in the case that matters.
    WhileOpen,
    /// In the background, which is the only setting a place cue works under.
    Always,
}

/// A place to watch for, as the platform needs it.
///
/// Coordinates are micro-degrees because that is what the event body holds —
/// integers, exact, and hashed into an event id. A float there would make the
/// id depend on two devices agreeing bit-for-bit about a decimal fraction, and
/// would put NaN inside a value that has to compare and converge.
#[derive(Debug, uniffi::Record)]
pub struct Geofence {
    /// The intent's event id, hex. This is what the platform hands back when
    /// the fence triggers, so it is what identifies the fence.
    pub id: String,
    pub lat_e6: i32,
    pub lon_e6: i32,
    pub radius_m: u32,
    /// What to say on arrival.
    pub then: String,
}

/// An if-then plan, as a surface lists it.
#[derive(Debug, uniffi::Record)]
pub struct Intent {
    pub id: String,
    /// `time`, `recurring`, `location`, `app_focus`, `git_branch`, …
    pub cue_type: String,
    /// What it matches on, where the cue takes one.
    pub cue_match: Option<String>,
    pub then: String,
    /// Whether *this* device can observe the cue at all.
    ///
    /// An intent nothing can watch is the failure §2.4.1 is careful about: it
    /// looks registered, is reported as working, and is permanently silent.
    pub observable_here: bool,
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

        // An intent that cannot fire, said out loud and kept being said.
        //
        // §7.3, and the reason it is `attention` rather than `worth-knowing`:
        // an intent failing invisibly is worse than no intent at all, because
        // the person believes something is watching for them when nothing is.
        // They stop holding it in mind — which is the entire thing this is for.
        let watching = ops.get(LOCATION_PERMITTED)?;
        let places = store
            .intents()?
            .into_iter()
            .filter(|i| i.cue_type == "location")
            .count();
        if places > 0 {
            // The middle case is the one worth spelling out. "While the app is
            // open" reads like a smaller version of permission and is in fact
            // no permission at all for this purpose: nobody is looking at a
            // notes application while walking into a shop.
            let trouble = match watching.as_deref() {
                Some("always") => None,
                Some("while-open") => Some(format!(
                    "{places} plan(s) about a place only fire while capture is open, \
                     which is not when you would be arriving anywhere. This device \
                     needs to see where it is in the background."
                )),
                _ => Some(format!(
                    "{places} plan(s) about a place cannot fire: this device is not \
                     allowed to see where it is. Nothing is watching for them."
                )),
            };
            if let Some(text) = trouble {
                out.push(Condition {
                    tier: "attention".into(),
                    text,
                });
            }
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

    /// Run one sync round, for at most this many seconds.
    ///
    /// The window is a parameter because the caller is about to be killed.
    /// Android runs this inside a job with a budget, and anything still going
    /// when the budget ends is destroyed mid-sentence rather than wound up.
    ///
    /// Blocking, and meant to be: the caller is a background worker whose
    /// whole purpose is to wait for this.
    ///
    /// Reaching nobody is not an error. It is the ordinary result of being the
    /// only device awake, and reporting it as a failure would teach a person
    /// to ignore the report.
    pub fn sync(&self, seconds: u32) -> Result<SyncReport> {
        let identity = capture_store::store::open(&self.paths)?;
        let r = capture_store::round::bounded(
            &self.paths,
            &identity,
            std::time::Duration::from_secs(seconds.clamp(1, 120) as u64),
            &|| jiff::Timestamp::now().as_millisecond().max(0) as u64,
        )?;
        Ok(SyncReport {
            multicast: r.multicast,
            answered: r.answered,
            trouble: r.trouble,
        })
    }

    /// Where another device can reach this one.
    ///
    /// Asked for because the other end has to be told. Pairing is a person
    /// typing a code into a second machine, and that machine needs an address
    /// — on a phone there is nowhere else to read one from, and "look it up in
    /// your network settings" is the point at which somebody gives up.
    ///
    /// Loopback is excluded: a device told to dial that would be dialling
    /// itself, which fails in a way that reads as the code being wrong.
    pub fn addresses(&self) -> Vec<String> {
        capture_store::round::local_addresses()
            .into_iter()
            .map(|a| a.to_string())
            .collect()
    }

    /// Every device this archive knows to talk to.
    ///
    /// The same union a sync round considers, and for the same reason: a
    /// device that has just been admitted has an address nowhere, because the
    /// port it paired from was ephemeral and it has never dialled in. Listing
    /// only remembered addresses would show *nothing* on a device seconds
    /// after pairing succeeded on it — which reads as pairing having failed,
    /// and is the moment a person is most likely to try again and make a
    /// mess.
    pub fn peers(&self) -> Result<Vec<Peer>> {
        let ops = capture_store::ops::Ops::open_read(&self.paths.ops)?;
        let me = capture_store::store::open(&self.paths)?.public().to_bytes();

        let known = ops.peer_addresses()?;
        let mut out: Vec<Peer> = Vec::new();
        let mut seen: Vec<[u8; 32]> = Vec::new();

        for p in known {
            if p.key == me {
                continue;
            }
            seen.push(p.key);
            out.push(Peer {
                key: named(&p.key),
                addr: p.addr,
                last_seen: p.last_seen.map(when),
                missed: p.attempts,
                observes: ops.peer_observes(&p.key)?,
            });
        }

        for key in ops.dialable()? {
            if key == me || seen.contains(&key) {
                continue;
            }
            out.push(Peer {
                key: named(&key),
                addr: None,
                last_seen: None,
                missed: 0,
                observes: ops.peer_observes(&key)?,
            });
        }

        Ok(out)
    }

    /// Offer this device for pairing.
    ///
    /// Two steps rather than one, because the code has to be readable while
    /// the window is open. A single call that returned the code would return
    /// it once pairing had already happened, which is the one moment it is of
    /// no use to anybody.
    ///
    /// **Emits no `setting`** — §8.9: pairing is a person connecting two
    /// devices, not a person expressing a preference, and an archive whose
    /// settings were written by the act of pairing could never afterwards say
    /// which of them anybody chose.
    pub fn offer_pairing(&self) -> Result<std::sync::Arc<Offer>> {
        let offering = capture_store::pairing::Offering::open(&self.paths)?;
        Ok(std::sync::Arc::new(Offer {
            code: offering.shown(),
            paths: self.paths.clone(),
            offering: Mutex::new(Some(offering)),
        }))
    }

    /// Join a device that is offering, by the code it is showing.
    ///
    /// Returns the device that vouched for this one — not the address it was
    /// reached at. An address is where something was a moment ago; the key is
    /// what was actually joined, and it is what every other surface names a
    /// device by.
    pub fn join_with_code(&self, addr: String, code: String) -> Result<String> {
        let _writing = self.write_lock()?;
        let now = jiff::Timestamp::now().as_millisecond().max(0) as u64;
        let grant = capture_store::pairing::accept_code(&self.paths, code.as_bytes(), &addr, now)?;
        Ok(named(&grant.pubkey))
    }

    /// Open a review session.
    ///
    /// Writes as well as reads: the archive records that these were offered,
    /// which is what makes the interval expand. So opening one twice in a row
    /// is not free, and the surface should open one when a person goes to
    /// review rather than when they happen to land near it.
    pub fn review(&self) -> Result<Review> {
        let _writing = self.write_lock()?;
        let now = jiff::Timestamp::now().as_millisecond().max(0);
        let offered = capture_store::review::open(&self.paths, now)?;
        Ok(Review {
            items: offered
                .items
                .into_iter()
                .map(|i| Resurfaced {
                    slug: i.slug,
                    text: i.text,
                    because: match i.because {
                        capture_store::review::Because::Forward => "forward".into(),
                        capture_store::review::Because::Never => "never-engaged".into(),
                        capture_store::review::Because::Context => "context".into(),
                    },
                    suggestion: i.suggestion,
                })
                .collect(),
            nagging: offered.nagging,
        })
    }

    /// What became of an offered note.
    ///
    /// `engaged` or `deferred`. Two, and not a grade — §2.7 removed the graded
    /// outcome deliberately, and the reason bears repeating at every surface
    /// that might be tempted to add one back: a permanent, repeating record of
    /// memory failures, delivered on a schedule the person did not set, to
    /// somebody whose stated difficulty is memory.
    ///
    /// `shown` is not accepted. That one is written by the archive when a
    /// session is opened, and a surface able to write it could inflate an
    /// interval without anybody having seen anything.
    pub fn reviewed(&self, name: String, outcome: String) -> Result<()> {
        let _writing = self.write_lock()?;
        let outcome = match outcome.as_str() {
            "engaged" => capture_core::body::Outcome::Engaged,
            "deferred" => capture_core::body::Outcome::Deferred,
            other => {
                return Err(CaptureError::Refused {
                    reason: format!("'{other}' is not an outcome a surface may record"),
                });
            }
        };
        capture_store::review::record(&self.paths, &name, outcome)?;
        Ok(())
    }

    /// Places to watch for, for the platform to register.
    ///
    /// Only the location intents, and only their coordinates: deciding *what
    /// to do* on arrival stays here. The platform's job is to notice a
    /// boundary being crossed, which is the one part of this a phone does
    /// better than anything else and the one part that must not be done by
    /// polling a radio.
    pub fn geofences(&self) -> Result<Vec<Geofence>> {
        let store = self.read()?;
        let mut out = Vec::new();
        for i in store.intents()? {
            if i.cue_type != "location" {
                continue;
            }
            // `lat,lon` as the projection writes it. The radius is not in the
            // projection's summary, so it comes from the body.
            let Some((lat, lon)) = i.cue_match.as_deref().and_then(|m| m.split_once(',')) else {
                continue;
            };
            let (Ok(lat_e6), Ok(lon_e6)) = (lat.parse::<i32>(), lon.parse::<i32>()) else {
                continue;
            };
            out.push(Geofence {
                id: hex(&i.id),
                lat_e6,
                lon_e6,
                radius_m: self.radius_of(&i.id).unwrap_or(DEFAULT_RADIUS_M),
                then: i.then_do,
            });
        }
        Ok(out)
    }

    /// The radius an intent asked for, read from its body.
    ///
    /// The projection keeps the coordinates and not the radius, because
    /// nothing on the desktop ever needed it. Read from the event rather than
    /// added to the projection: the schema may add tables and never columns,
    /// and one field for one caller is not worth a table.
    fn radius_of(&self, id: &[u8]) -> Option<u32> {
        let log = capture_store::log::Log::open_read(&self.paths.events).ok()?;
        let bytes: [u8; 32] = id.try_into().ok()?;
        let event = log.by_id(&capture_core::EventId(bytes)).ok()??;
        let body =
            minicbor::decode::<capture_core::body::IntentBody>(event.body.as_deref()?).ok()?;
        match body.cue {
            capture_core::body::Cue::Location { radius_m, .. } => Some(radius_m),
            _ => None,
        }
    }

    /// A fence was crossed. What, if anything, to say.
    ///
    /// `None` where this fired recently enough that saying it again would be
    /// noise: a shop is a point with a radius, somebody at the counter is
    /// metres from its edge, and a phone re-acquiring its position crosses
    /// that edge without anybody moving.
    ///
    /// Marking it as fired happens here rather than in the caller, so that a
    /// surface which forgot cannot produce a notification every few minutes.
    pub fn arrived(&self, id: String) -> Result<Option<String>> {
        let Some(raw) = unhex(&id) else {
            return Err(CaptureError::NotFound { name: id });
        };
        let store = self.read()?;
        let Some(intent) = store.intents()?.into_iter().find(|i| i.id == raw) else {
            // Retracted while the fence was still registered with the
            // platform. Not an error: the answer is simply that there is
            // nothing to say, and the caller should stop watching for it.
            return Ok(None);
        };

        let now = jiff::Timestamp::now().as_millisecond().max(0) as u64;
        let ops = capture_store::ops::Ops::open(&self.paths.ops)?;
        if !capture_store::notifier::may_fire_again(
            ops.intent_fired_at(&raw)?,
            now,
            &intent.cue_type,
        ) {
            return Ok(None);
        }
        ops.mark_intent_fired(&raw, now)?;
        Ok(Some(intent.then_do))
    }

    /// Make a plan about a place.
    ///
    /// Refused where this device cannot observe a geofence, which is every
    /// device that does not move. That refusal is the point of §2.4.1: an
    /// intent registered somewhere that cannot see its cue is one that looks
    /// like it is working and never fires.
    pub fn watch_place(
        &self,
        lat_e6: i32,
        lon_e6: i32,
        radius_m: u32,
        then: String,
    ) -> Result<String> {
        let _writing = self.write_lock()?;
        let id = capture_store::mutate::add_intent(
            &self.paths,
            capture_core::body::Cue::Location {
                lat_e6,
                lon_e6,
                radius_m,
            },
            &then,
        )?;
        Ok(hex(&id.0))
    }

    /// Every plan, and whether this device can see its cue.
    pub fn intents(&self) -> Result<Vec<Intent>> {
        let store = self.read()?;
        Ok(store
            .intents()?
            .into_iter()
            .map(|i| Intent {
                id: hex(&i.id),
                observable_here: capture_store::notifier::observable(&i.cue_type),
                cue_type: i.cue_type,
                cue_match: i.cue_match,
                then: i.then_do,
            })
            .collect())
    }

    /// Give one up.
    ///
    /// Retracted rather than removed: a plan somebody walked away from is a
    /// fact about what they tried, and §2.4 counts being easy to abandon as
    /// part of the case for having intents at all.
    pub fn retract_intent(&self, id: String) -> Result<()> {
        let _writing = self.write_lock()?;
        let Some(raw) = unhex(&id) else {
            return Err(CaptureError::NotFound { name: id });
        };
        let bytes: [u8; 32] = raw
            .try_into()
            .map_err(|_| CaptureError::NotFound { name: id })?;
        let event = capture_core::EventId(bytes);
        capture_store::mutate::retract_intent(&self.paths, &event)?;
        Ok(())
    }

    /// The device the user chose to be told by, if they chose one.
    ///
    /// `None` means nobody has chosen — not that nothing notifies. §8.9: the
    /// unchosen case is decided by a rule and written nowhere, and the
    /// difference between that and a choice is what makes the choice legible
    /// later.
    pub fn notifier(&self) -> Result<Option<String>> {
        Ok(capture_store::notifier::chosen(&self.read()?)?)
    }

    /// Choose which device says things.
    pub fn set_notifier(&self, device: Option<String>) -> Result<()> {
        let _writing = self.write_lock()?;
        capture_store::mutate::set_setting(
            &self.paths,
            capture_store::notifier::KEY,
            device.as_deref(),
        )?;
        Ok(())
    }

    /// Say how much of the time this device may observe where it is.
    ///
    /// Recorded locally rather than in the log: a permission is a fact about
    /// this installation on this handset, not about the archive, and §2.5.2
    /// keeps that out of a log meant to be worth reading in ten years.
    ///
    /// It is recorded at all so that [`Self::health`] can say a place cue
    /// cannot fire, and keep saying it. An intent failing invisibly is worse
    /// than no intent: somebody believes something is watching for them, stops
    /// holding it in mind, and that is the whole thing this is for.
    pub fn location_permitted(&self, watching: Watching) -> Result<()> {
        let ops = capture_store::ops::Ops::open(&self.paths.ops)?;
        ops.set(
            LOCATION_PERMITTED,
            match watching {
                Watching::Never => "never",
                Watching::WhileOpen => "while-open",
                Watching::Always => "always",
            },
        )?;
        Ok(())
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

/// A pairing window that is open, and the code that closes it.
///
/// Held as an object rather than returned as a pair of values because the
/// window is a live socket: dropping this closes it, which is the behaviour
/// wanted when a person navigates away mid-pairing.
#[derive(uniffi::Object)]
pub struct Offer {
    code: String,
    paths: StorePaths,
    /// Taken by the first `wait`, because admitting consumes the listener.
    offering: Mutex<Option<capture_store::pairing::Offering>>,
}

#[uniffi::export]
impl Offer {
    /// The six digits to type on the joining device.
    pub fn code(&self) -> String {
        self.code.clone()
    }

    /// Block until a device joins, or the window closes.
    ///
    /// Returns the joined device's key. Called from a background thread on the
    /// caller's side: this waits for a person to walk to another machine.
    pub fn wait(&self) -> Result<String> {
        let offering = self
            .offering
            .lock()
            .map_err(|_| CaptureError::Refused {
                reason: "the pairing window was left in an unknown state".into(),
            })?
            .take()
            .ok_or_else(|| CaptureError::Refused {
                reason: "this pairing window has already been used".into(),
            })?;

        let now = || jiff::Timestamp::now().as_millisecond().max(0) as u64;
        let joined = offering.admit_one(&self.paths, now(), now)?;
        Ok(joined[..6].iter().map(|b| format!("{b:02x}")).collect())
    }
}

/// The first bytes of a key, which is how every surface names a device.
/// Where the location permission is remembered.
const LOCATION_PERMITTED: &str = "android.location_permitted";

/// The radius used when the intent did not name one.
///
/// A hundred metres: large enough that GPS error does not stop a fence
/// triggering at all, small enough that "at the hardware shop" is not "in the
/// retail park". The platform will not accept much less reliably anyway.
const DEFAULT_RADIUS_M: u32 = 100;

fn hex(raw: &[u8]) -> String {
    raw.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || s.is_empty() {
        return None;
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok())
        .collect()
}

fn named(key: &[u8; 32]) -> String {
    key[..6].iter().map(|b| format!("{b:02x}")).collect()
}

/// A moment, in this device's zone.
fn when(ms: u64) -> String {
    let here = jiff::tz::TimeZone::system();
    let (day, at) = rendered(ms as i64, here.iana_name());
    format!("{day} {at}")
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
