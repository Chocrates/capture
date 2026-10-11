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

/// A moment that has come due, and when it may be said.
#[derive(Debug, uniffi::Record)]
pub struct Alert {
    /// The note, so tapping the notification can open it.
    pub slug: String,
    /// What to say: the capture's own text (§3.1.3).
    pub text: String,
    /// When it came due.
    pub at_ms: i64,
    /// Which moment of that note, for a series. Absent for a one-off.
    pub occ: Option<i64>,
    /// Whether the capture named a time of day rather than only a day. §2.6
    /// delivers this one immediately and batches the rest.
    pub timed: bool,
    /// Whether this moment was already delivered. A surface showing what is
    /// coming needs it; a surface about to deliver checks it.
    pub delivered: bool,
}

/// When the alarm should next go off, and whether that moment earns an exact
/// alarm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct Wake {
    /// The instant to wake at.
    pub at_ms: i64,
    /// Whether this is a moment somebody named a time for, rather than a hold
    /// being released or a retry — the only case worth the exact-alarm
    /// permission.
    pub exact: bool,
}

/// One line of a list: an item written into a list capture, or a task that
/// carries the list's tag.
///
/// The two arrive as one shape on purpose. To whoever is holding the phone in
/// a shop they are the same thing — something on the list, bought or not — and
/// which of the two it is underneath is not theirs to keep track of.
#[derive(Debug, uniffi::Record)]
pub struct ListLine {
    /// What to draw.
    pub text: String,
    /// Whether it has been ticked off.
    pub done: bool,
    /// Its place in the order somebody wrote, where it has one. Absent for a
    /// task that joined the list by carrying its tag.
    pub ord: Option<u32>,
    /// The capture this line came from.
    ///
    /// A collection gathers items from however many captures wrote them, and a
    /// surface needs to know which — to tick the right one by position, and to
    /// say that what is on screen is not one person's single list.
    pub slug: String,
}

/// What ticking did, so the surface can say it rather than guess.
///
/// An enum rather than a bool: "already done" is not a failure and "two items
/// read the same" is not success, and a phone that showed one word for all
/// four would be lying in two of them.
#[derive(Debug, uniffi::Enum)]
pub enum Ticked {
    /// It carries the state asked for now, and did not before.
    Done,
    /// It already did. The surface can stay quiet rather than claim it acted.
    Already,
    /// More than one item on this list reads the same. Completion matches an
    /// item by its text, so these cannot be told apart.
    Ambiguous { count: u32 },
    /// Nothing on this list reads like that.
    NoSuchItem,
}

/// One marker a capture can carry, as the field shows it.
#[derive(Debug, uniffi::Record)]
pub struct Marker {
    /// The character that opens it.
    pub sigil: String,
    /// What a person would write after it.
    pub example: String,
    /// What it does.
    pub means: String,
}

/// What can be written in a capture, for showing under the field.
///
/// A glance, not a lesson. §3.1.1 settles that the input language is English
/// and that the markers are discovered by seeing them — which requires that
/// they are somewhere to be seen, and on a phone they never have been.
///
/// Read from the vocabulary rather than written out here. A second copy on the
/// surface would drift from the one the recognizer uses, and the drift would
/// show up as a hint offering something that no longer works.
#[derive(Debug, uniffi::Record)]
pub struct Vocabulary {
    /// The sigils, each with an example of what follows it.
    pub markers: Vec<Marker>,
    /// Shapes the grammar reads, as examples rather than rules.
    pub shapes: Vec<String>,
}

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
    /// What this capture is, as the projection reads it: `note`, `task`,
    /// `event` or `series`. A capture carrying no promotion answers `note`.
    pub kind: String,
    /// The projection's one-line rendering of the fields — "done · due
    /// 2026-09-20", "Thu 14:00", "every weekday at 9".
    ///
    /// Empty for an ordinary note, deliberately: its content is its text, and
    /// a descriptor invented for one would add a fact the capture did not
    /// contain (§3.1.3). A surface draws the text alone when this is empty.
    ///
    /// Supplied here rather than composed by each surface. Two surfaces
    /// formatting the same fields would be two renderings that can disagree,
    /// and each would look correct on its own screen.
    pub descriptor: String,
    /// The items this capture names, where it is a list. Empty otherwise.
    ///
    /// On the summary rather than fetched per row: a timeline of a hundred
    /// rows would otherwise make a hundred calls across this boundary to
    /// discover that ninety-eight of them are not lists.
    pub items: Vec<ListLine>,
    /// The tag this note's text opens a list under, where it does.
    ///
    /// Decided here rather than on the surface. The phone drawing a "open the
    /// list" affordance has to know *whether this is a list*, and that is a
    /// question about the recognizer — Kotlin re-answering it would be a
    /// second implementation of the form, differing from this one the first
    /// time a guard changed.
    pub list_tag: Option<String>,
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

/// A recording attached to a note.
#[derive(Debug, uniffi::Record)]
pub struct Recording {
    /// Its content-addressed name, for asking any device that has the bytes.
    pub name: String,
    pub media_type: String,
    pub bytes: u64,
    pub duration_ms: Option<u32>,
    /// Whether *this* device holds the bytes.
    ///
    /// False is ordinary rather than broken: a phone evicts aggressively
    /// (§5.6) and the bytes come back on the next round with a device that
    /// still has them. A surface should say "not on this device" and offer to
    /// fetch, not report a missing recording.
    pub here: bool,
    /// For how many whole days every device this one reached has said it does
    /// not have the bytes — set only once that has held for enough rounds over
    /// enough time that "the other device is asleep" has stopped being the
    /// likely reason.
    ///
    /// `None` while `here`, and while a miss is still ordinary. Decided in the
    /// store and counted in days here, so the surface only chooses words and
    /// does no date arithmetic of its own.
    pub unavailable_for_days: Option<u32>,
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

/// One thing happening on a day: an occurrence of a series, or a task due.
///
/// A list rather than a column-packed grid. The desktop packs overlapping
/// entries into columns because it has the width for it; a phone agenda is
/// read top to bottom, and packing would be work done to produce a layout
/// nothing renders.
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
    /// When it ends, where that was written down.
    pub end_ms: Option<i64>,
    /// Which column it sits in, among things it overlaps (§4.4).
    pub column: u32,
    /// How many columns its cluster is wide, so it can be drawn at `1/columns`.
    ///
    /// The cluster decides, not the day: a single three-way overlap at noon
    /// must not squeeze the morning into thirds.
    pub columns: u32,
}

/// Where an open task sits on a surface asking what is outstanding.
///
/// Mirrors `capture_store::projection::Band`. The band is decided by the store
/// against one answer to what today is, so that a phone and a window cannot
/// disagree about whether something is late.
#[derive(Debug, PartialEq, uniffi::Enum)]
pub enum TaskBand {
    /// Due on a day that has passed.
    Overdue,
    /// Due today or later.
    Due,
    /// Open, with no due date. The ordinary case: a bare task verb is below the
    /// confidence floor, so most tasks are named by a person, and naming one is
    /// not saying when.
    Undated,
}

/// One open task.
///
/// Finished tasks are not here at all. Nothing counts completions (§0 rates
/// nothing that scores the user), so there is no done band to hold them.
#[derive(Debug, uniffi::Record)]
pub struct TaskLine {
    pub slug: String,
    /// What was captured, verbatim. Absent for the same reasons as elsewhere:
    /// redacted, queued for redaction, or not yet readable.
    pub text: Option<String>,
    /// The projection's one-line rendering, so this surface cannot format the
    /// fields differently from the timeline or the note page.
    pub descriptor: String,
    /// The due date as a floating day, `2026-09-20`, never an instant.
    pub due: Option<String>,
    pub band: TaskBand,
}

/// A person, a project or a tag, and everything its page shows.
///
/// Whole rather than in parts. A page is a display name, the spellings that
/// reach it, the note that *is* it and the notes naming it — and three surfaces
/// assembling those separately is three answers to one question, which is the
/// argument that put the descriptor in the projection.
#[derive(Debug, uniffi::Record)]
pub struct Entity {
    /// What to call it: the name somebody chose, or the earliest spelling.
    pub display: String,
    /// person | project | tag.
    pub kind: String,
    /// Every written spelling that reaches it, folded. More than one after a
    /// merge, which is how a page says what it has also been called.
    pub names: Vec<String>,
    /// The note that *is* this entity, where one is bound. Shown above the rest
    /// and absent from them.
    pub backing: Option<NoteSummary>,
    /// The notes naming it, newest first.
    pub notes: Vec<NoteSummary>,
}

/// One run of a note's text: ordinary words, or a mention that leads somewhere.
///
/// The split is done here rather than on the surface. Which runs are mentions is
/// the same question `mentions` answers for the projection, and a phone scanning
/// for sigils itself would be a second implementation of it — one that would
/// drift the first time the rule changed, and would disagree about
/// `sarah@example.com` long before that.
#[derive(Debug, uniffi::Record)]
pub struct TextPiece {
    /// The text of this run, exactly as written.
    pub text: String,
    /// The mention it is, as written — `@sarah`. `None` for ordinary text.
    pub mention: Option<String>,
}

/// What a capture is being said to be.
///
/// An enum rather than the string `NoteSummary.kind` carries. That one is a
/// rendering, where a wrong value draws badly; this one is a mutation, where a
/// wrong value writes a pinned correction that outlives every later reparse.
/// Kotlin should not be able to name a kind that does not exist.
#[derive(Debug, PartialEq, uniffi::Enum)]
pub enum NoteKind {
    /// Explicitly neither a task nor an event. A demotion is a promotion like
    /// any other, which is what makes a wrong type correctable rather than
    /// something to be worked around.
    Note,
    /// Something to do. State, and no time of its own.
    Task,
    // No `Event` arm, and its absence is deliberate rather than unfinished.
    //
    // `Promoted::Event` carries `EventFields`, which begins with a `When` — an
    // event is a thing that occupies time, so there is no such thing as one
    // without a time (§3.6). A bare "make this an event" has nothing to put
    // there. Saying when is how something becomes an event, which is what the
    // capture text and the recognizer already do.
}

impl capture_store::calendar::Timed for AgendaEntry {
    fn start_ms(&self) -> i64 {
        self.start_ms
    }

    fn end_ms(&self) -> Option<i64> {
        self.end_ms
    }

    fn on_axis(&self) -> bool {
        !self.all_day && !self.due
    }

    fn place(&mut self, column: usize, columns: usize) {
        self.column = column as u32;
        self.columns = columns as u32;
    }
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
    /// The platform permission whose absence this is, where it is one:
    /// `notifications` or `exact_alarms`.
    ///
    /// So a surface that lists permissions with a button each can leave these
    /// out of its list of problems rather than saying the same thing twice in
    /// two places — and can tell which ones a single tap fixes, without
    /// matching on the wording.
    pub permission: Option<String>,
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

            // A projection whose shape predates this build.
            //
            // §8.8 makes the projection derived and nothing in it
            // authoritative, which is what allows the migration strategy to be
            // "throw it away and compute it again". `Session::open` already
            // does this — but the phone's reading path does not open a
            // session, it opens the current generation directly, so a cold
            // launch after an upgrade read the *old* shape and failed with a
            // missing column before anything had a chance to notice.
            //
            // Here rather than in `read`, which runs per query: this costs a
            // full fold and fires once per upgrade.
            let stale = capture_store::projection::current(&paths)
                .and_then(|store| store.is_stale())
                .unwrap_or(false);

            if stale {
                let mut s = capture_store::session::Session::open(paths.clone())?;
                s.rebuild_projection()?;
            }
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

    /// What can be written in a capture.
    ///
    /// Constant for the life of a build, so a surface may read it once and
    /// keep it. It is a call rather than a constant so that the vocabulary has
    /// exactly one definition — growing it is an edit in `capture-parse` with
    /// tests attached (§3.1.5), and a hardcoded copy on a surface would be a
    /// second place to forget.
    pub fn vocabulary(&self) -> Vocabulary {
        Vocabulary {
            markers: capture_parse::SIGILS
                .iter()
                .map(|s| Marker {
                    sigil: s.sigil.to_string(),
                    example: s.example.to_string(),
                    means: s.means.to_string(),
                })
                .collect(),
            shapes: core::iter::once(capture_parse::LIST_SHAPE.to_string())
                .chain(capture_parse::SHAPES.iter().map(|s| s.to_string()))
                .collect(),
        }
    }

    /// Record whether this device is allowed to show notifications at all.
    ///
    /// Told by the surface, because it is an Android fact. Reported on the
    /// health surface rather than only attempted, since the platform's way of
    /// refusing is to do nothing.
    pub fn set_alerts_silenced(&self, silenced: bool) -> Result<()> {
        let ops = capture_store::ops::Ops::open(&self.paths.ops)?;
        ops.set("alerts.silenced", if silenced { "1" } else { "0" })?;
        Ok(())
    }

    /// Record whether this device can schedule an exact alarm.
    ///
    /// Local state rather than a `setting` event: it is a fact about this
    /// phone's permissions, not a decision about the archive, and syncing it
    /// would tell the desktop something untrue about itself (§2.5.2).
    pub fn set_alerts_inexact(&self, inexact: bool) -> Result<()> {
        let ops = capture_store::ops::Ops::open(&self.paths.ops)?;
        ops.set("alerts.inexact", if inexact { "1" } else { "0" })?;
        Ok(())
    }

    /// Whether this device failed to schedule the next moment.
    ///
    /// Only one alarm is ever outstanding and each firing schedules the next, so
    /// the chain is the entire alerting mechanism: a single arming that does not
    /// happen ends it, and nothing later notices, because what would have
    /// noticed is the alarm that was never set.
    ///
    /// Recorded rather than retried away, because the retry can fail too. Every
    /// other alert condition here describes a platform refusal that at least
    /// left the machinery running; this one says the machinery stopped.
    pub fn set_alerts_unscheduled(&self, unscheduled: bool) -> Result<()> {
        let ops = capture_store::ops::Ops::open(&self.paths.ops)?;
        ops.set("alerts.unscheduled", if unscheduled { "1" } else { "0" })?;
        Ok(())
    }

    fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(capture_store::ops::Ops::open_read(&self.paths.ops)?.get(key)?)
    }

    /// How many captures nothing has read yet.
    ///
    /// Read-only and no writer involved, which is the whole point: a surface
    /// has to be able to say "this may be incomplete" at the moment the writer
    /// has not been running, and a report that needed the writer could not.
    pub fn unread(&self) -> Result<u32> {
        Ok(self.read()?.unread_count(capture_parse::GRAMMAR_VERSION)? as u32)
    }

    /// What has come due by a moment, oldest first.
    ///
    /// Operation-shaped: the caller says when, and gets back what may be said
    /// and how soon. It does not deliver — asking twice answers twice, which
    /// is what lets a surface show what is coming without announcing it.
    ///
    /// `within_ms` looks ahead. Zero is "what has arrived", which is what the
    /// alarm receiver asks; a week is what a screen showing the schedule asks.
    pub fn due(&self, within_ms: i64) -> Result<Vec<Alert>> {
        // Reading first, because §3.1 puts parsing after the commit: a capture
        // made a moment ago is not yet a promotion, and a promotion is what
        // carries the time. Without this the alarm that fires two minutes after
        // a capture would find nothing due and be right.
        //
        // Load-bearing on this platform specifically. There is no sync daemon
        // on Android, so nothing else is running to do this; the alarm receiver
        // arrives in the background with no guarantee the foreground pass has
        // happened. So [`Archive::settle`] here as well as on foreground — that
        // covers the screens, and this covers the alarm.
        //
        // Through `settle` rather than opening a throwaway session inline,
        // which is what this was. One named operation, so a third caller that
        // needs the same thing finds it instead of growing its own.
        self.settle()?;

        let store = self.read()?;
        let ops = capture_store::ops::Ops::open_read(&self.paths.ops)?;
        let now = self.now_ms() as i64;

        store
            .due_by(now + within_ms, now, &self.zone_name())?
            .into_iter()
            .map(|d| {
                Ok(Alert {
                    slug: store.slug_of(&d.tgt)?.unwrap_or_default(),
                    text: d.text,
                    at_ms: d.at,
                    occ: d.occ,
                    timed: d.timed,
                    delivered: ops.alerted(&d.tgt.0, d.occ, d.at)?,
                })
            })
            .collect()
    }

    /// Remember that a moment was delivered, so it is not said twice.
    ///
    /// Local state rather than the log (§2.5.2): which device buzzed and when
    /// is operational chatter, and an archive restored on a new machine must
    /// not replay a year of notifications.
    ///
    /// `at_ms` is the moment delivered, not the moment of delivery — it comes
    /// straight back from the alert the caller was handed. It is part of the
    /// key: a one-off carries no occurrence, so without it "this note was
    /// alerted" and "this moment was alerted" are the same record, and
    /// correcting a time after the first alert fired silences the corrected
    /// moment for good.
    pub fn delivered(&self, slug: String, occ: Option<i64>, at_ms: i64) -> Result<()> {
        let store = self.read()?;
        let tgt = store
            .resolve(&slug)?
            .ok_or(CaptureError::NotFound { name: slug })?;
        capture_store::ops::Ops::open(&self.paths.ops)?.mark_alerted(
            &tgt.0,
            occ,
            at_ms,
            self.now_ms(),
        )?;
        Ok(())
    }

    /// Whether a moment is still recent enough to be worth announcing.
    ///
    /// Asked before `hold_until`, and separately from it, because "say this
    /// now", "say this at seven" and "do not say this" are three answers and an
    /// optional instant can only carry two.
    ///
    /// The surface still lists it. This is only about interrupting somebody.
    pub fn too_late_to_say(&self, late_by_ms: i64) -> bool {
        late_by_ms > capture_store::notifier::STALE_MS
    }

    /// Whether something due now may be said now, or must wait — and until
    /// which local hour.
    ///
    /// The budget of §2.6, answered here rather than on the surface. A phone
    /// deciding its own quiet hours would be a second implementation of a rule
    /// that has to agree with the desktop's, and the two would drift the first
    /// time either changed.
    ///
    /// Returns the instant to wait until, or `None` to say it now.
    ///
    /// An instant rather than the hour `deliver` works in. That function is
    /// pure and stays that way — §8.1 keeps clocks out of `capture-store`, and
    /// an hour is all a rule needs. Turning "07:00" into a moment needs to know
    /// what day it is and which zone, and this binding is the edge where both
    /// are known.
    ///
    /// It also keeps the arithmetic out of the surface. A phone working out
    /// what instant 07:00 tomorrow is would be doing date arithmetic in
    /// Kotlin, which `make android-decides` refuses — and rightly: it is the
    /// same calculation the core already does, differing at the edges nobody
    /// looks at until a clock goes back.
    pub fn hold_until(&self, timed: bool, armed: bool, late_by_ms: i64) -> Result<Option<i64>> {
        let now = self.now_ms() as i64;
        let zone = resolve_zone(&self.zone_name())?;
        let hour = capture_time::civil_clock(now, zone).0;

        let until = match capture_store::notifier::deliver_late(hour, timed, armed, late_by_ms) {
            capture_store::notifier::Delivery::Now => return Ok(None),
            capture_store::notifier::Delivery::At(h) => h,
            // A hold, so that a caller which did not ask first still does the
            // harmless thing rather than announcing it. The answer a surface
            // wants for this case is [`Self::too_late_to_say`], which it is
            // expected to ask before this — the two are separate because they
            // are separate questions, and collapsing them into one optional
            // instant is what made "say it now" and "never say it" the same
            // answer.
            capture_store::notifier::Delivery::Stale => capture_store::notifier::next_batch(hour),
        };
        at_hour(now, zone, until).map(Some)
    }

    /// When the alarm should next go off, if at all.
    ///
    /// # Why this is not "the next undelivered moment"
    ///
    /// It was, on the phone: the alarm was armed for the first moment not yet
    /// delivered, and one already past was armed for a second from now. But a
    /// moment can stay undelivered on purpose — held to morning by quiet hours,
    /// held to the next batch, or too old to announce at all — and each of
    /// those came round a second later, was held again, and armed again. A
    /// Pixel left with one invoice from a fortnight before woke 20,105 times in
    /// an hour and a half, every 0.3 seconds, as the top waker on the phone.
    ///
    /// So the question is asked of each moment the way delivery will ask it,
    /// and the answer is when delivery will next *do* something about it:
    ///
    /// - still ahead: its own instant, exact if it named a time of day;
    /// - due now: `soonest_ms` from now — a second when arming after a capture
    ///   or on resume, and minutes when arming after a ring that could not post
    ///   it, so a blocked channel is a handful of wakes rather than a loop;
    /// - held: the instant the hold ends;
    /// - too old to say: never. It is still listed; it will not interrupt.
    pub fn next_wake(&self, within_ms: i64, soonest_ms: i64) -> Result<Option<Wake>> {
        let alerts = self.due(within_ms)?;
        wake_for(
            &alerts,
            self.now_ms() as i64,
            resolve_zone(&self.zone_name())?,
            soonest_ms,
        )
    }

    /// The most recent notes, newest first.
    pub fn timeline(&self, limit: u32) -> Result<Vec<NoteSummary>> {
        let store = self.read()?;
        store
            .timeline(limit as usize)?
            .into_iter()
            .map(|n| read_as(&store, n))
            .collect()
    }

    /// One note by slug or alias.
    pub fn note(&self, name: String) -> Result<NoteSummary> {
        let store = self.read()?;
        let tgt = store
            .resolve(&name)?
            .ok_or_else(|| CaptureError::NotFound { name: name.clone() })?;
        let row = store.note(&tgt)?.ok_or(CaptureError::NotFound { name })?;
        read_as(&store, row)
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

    /// The zone every time decision on this device is made in.
    ///
    /// One answer, used by everything here that has to turn a day into a
    /// window. Two callers deriving it separately is how `today()` and
    /// `agenda()` came to disagree about what day it was.
    ///
    /// The platform's answer wins where a platform reported one. On Linux the
    /// zone is discoverable — `$TZ`, or what `/etc/localtime` points at — and
    /// `local_zone` reads it afresh on every call. Android exposes neither, so
    /// the library's answer there is whatever it read when the process started
    /// and stays that until the process dies. A phone is the device most likely
    /// to be carried across a time zone and the least likely to be restarted
    /// afterwards.
    ///
    /// So the surface reports it, the same way it reports whether notifications
    /// are permitted: a platform fact the core cannot ask about, written down
    /// where the core can read it.
    fn zone_name(&self) -> String {
        if let Ok(Some(z)) = self.setting(DEVICE_ZONE)
            && capture_time::Zone::new(&z).is_some()
        {
            return z;
        }
        capture_store::notifier::local_zone()
    }

    /// Say which zone this device is in.
    ///
    /// Called at launch and whenever the platform announces a change. Refused
    /// rather than stored if it does not resolve: a name nothing can parse
    /// would silently become UTC at every reader, which on most of the planet
    /// is a whole-day error at the edges.
    pub fn set_zone(&self, name: String) -> Result<()> {
        if capture_time::Zone::new(&name).is_none() {
            return Err(CaptureError::Refused {
                reason: format!("'{name}' is not a zone this device can resolve"),
            });
        }
        capture_store::ops::Ops::open(&self.paths.ops)?.set(DEVICE_ZONE, &name)?;
        Ok(())
    }

    /// Today, in this device's own zone.
    ///
    /// Asked for rather than computed by the caller, for the same reason the
    /// agenda's date is parsed here: what day it is depends on a zone, and
    /// there is one implementation of that.
    pub fn today(&self) -> String {
        // The same zone as everything else. Reading the library's directly is
        // how `today()` and `agenda()` came to disagree about what day it was.
        let zone = capture_time::Zone::new(&self.zone_name())
            .or_else(|| capture_time::Zone::new("UTC"))
            .expect("UTC resolves");
        let d = capture_time::civil_date(jiff::Timestamp::now().as_millisecond().max(0), zone);
        format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
    }

    /// What is happening on a day, and what is due.
    ///
    /// The date is a string like `2026-09-15` and is interpreted here rather
    /// than by the caller. §3.6 and the time model decide what a day is,
    /// including the daylight-saving cases, and a second answer to that
    /// question is a calendar that disagrees with itself.
    pub fn agenda(&self, day: String) -> Result<Vec<AgendaEntry>> {
        let store = self.read()?;
        // The zone the person is in, not UTC.
        //
        // `today()` answers in the system zone, so a day string arriving here
        // names a *local* day — and turning it into a UTC window put the day
        // boundary in the wrong place by the offset. West of Greenwich that
        // meant the agenda went empty for the last hours of every evening:
        // 19:00 in Chicago is already tomorrow in UTC, so everything happening
        // today fell outside the window today was asking for.
        //
        // Falls back to UTC only where the system zone has no IANA name, which
        // is the one case where there is nothing better to use.
        let zone = capture_time::Zone::new(self.zone_name().as_str())
            .or_else(|| capture_time::Zone::new("UTC"))
            .expect("UTC is always present");
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
                end_ms: None,
                column: 0,
                columns: 1,
            });
        }

        // `happening_between`, not `occurring_between`. The second is series
        // expansions only: a one-off event lives in `promotions` and gains an
        // occurrence row only once somebody writes an exception against it. So
        // the phone's agenda showed recurring things and silently omitted
        // every single-instance event — "dentist thursday 2pm", promoted at
        // 85% confidence, on no calendar at all. Both other surfaces already
        // called the right one; this was the third implementation of the same
        // question and the one that got it wrong.
        for (tgt, occ) in store.happening_between(from, to)? {
            out.push(AgendaEntry {
                slug: store.slug_of(&tgt)?.unwrap_or_default(),
                text: store.text_of(&tgt)?,
                start_ms: occ.start_ms,
                all_day: occ.all_day,
                due: false,
                done: occ.done,
                end_ms: occ.end_ms,
                column: 0,
                columns: 1,
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
                end_ms: None,
                column: 0,
                columns: 1,
            });
        }

        // Packed here, by the same code the desktop runs. Two implementations
        // would be two answers to "do these overlap", both plausible, neither
        // checked against the other, and visible only as two devices
        // disagreeing about what Tuesday looks like.
        capture_store::calendar::pack(&mut out);

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
            // A collision is not an authentication failure and must not be
            // described as one. The event verified under its origin's own key —
            // it was genuinely authored by a device holding that key — and what
            // is wrong is that two different events claim one position in that
            // origin's sequence, which means the origin forked. Filing it under
            // "failed authentication" points at the one explanation that is
            // definitely not the case.
            // Neither of these is an authentication failure, and describing
            // them as one points at the single explanation that is definitely
            // not the case. Both events verified under their origin's own key.
            if reason == "post_retire" {
                out.push(Condition {
                    tier: "worth-knowing".into(),
                    permission: None,
                    text: format!(
                        "{n} event(s) arrived from a device that was retired, authored \
                         after the point the retirement named. They are kept, unused, \
                         and are re-admitted only by reissuing the retirement."
                    ),
                });
                continue;
            }

            if reason == "collision" {
                out.push(Condition {
                    tier: "attention".into(),
                    permission: None,
                    text: format!(
                        "{n} event(s) arrived claiming a position this device has already                          filled — two devices have been writing as the same origin. The                          arriving events were not merged and their contents are kept."
                    ),
                });
                continue;
            }
            out.push(Condition {
                tier: "attention".into(),
                permission: None,
                text: format!("{n} event(s) failed authentication ({reason})"),
            });
        }

        // Whether this device can say a thing at the moment it names.
        //
        // The surface reports it here rather than the archive discovering it:
        // `SCHEDULE_EXACT_ALARM` is an Android fact, and a core that tried to
        // ask about it would be a core that knows what Android is.
        //
        // Said out loud rather than degraded quietly. An alert firing at some
        // point in the following half hour is a different product from one
        // firing at 2pm, and somebody relying on the second while receiving
        // the first has no way to find that out.
        // The one that makes every other alert moot.
        //
        // Android stops showing the permission prompt after it has been
        // dismissed twice, and `launch()` then does nothing at all — no
        // dialog, no callback, no error. An application that only asked
        // through the prompt would be permanently unable to speak and
        // permanently unable to say so.
        if self.setting("alerts.silenced")?.as_deref() == Some("1") {
            out.push(Condition {
                tier: "attention".into(),
                permission: Some("notifications".into()),
                text: "this device cannot show notifications, so nothing will be said when a \
                       time arrives. Allow them under Settings"
                    .into(),
            });
        }

        if self.setting("alerts.inexact")?.as_deref() == Some("1") {
            out.push(Condition {
                tier: "attention".into(),
                permission: Some("exact_alarms".into()),
                text: "alerts fire approximately — this device has not been allowed exact \
                       alarms, so a time may be up to an hour late. Allow alarms & reminders \
                       under Settings"
                    .into(),
            });
        }

        // The chain, before anything about the quality of what it delivers.
        //
        // "Alerts fire approximately" is a different statement from "no alert
        // will fire", and the two were indistinguishable: `alerts.silenced` and
        // `alerts.inexact` are written only from inside the arming path, so when
        // that path stopped running both keys kept whatever they last held —
        // almost certainly the values meaning everything is fine — and this
        // surface went on reporting a healthy alerting system with no alarm
        // behind it.
        if self.setting("alerts.unscheduled")?.as_deref() == Some("1") {
            out.push(Condition {
                tier: "attention".into(),
                permission: None,
                text: "nothing is scheduled to wake this device, so no time will be \
                       announced until capture is opened again"
                    .into(),
            });
        }

        if capture_store::spool::in_use(&self.paths.spool)? {
            out.push(Condition {
                tier: "attention".into(),
                permission: None,
                text: "captures are being spooled — the store could not be written".into(),
            });
        }

        for (_id, owed) in ops.redactions_outstanding()? {
            out.push(Condition {
                tier: "attention".into(),
                permission: None,
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
                    permission: None,
                    text,
                });
            }
        }

        // Whether the archive is still reaching the other device.
        //
        // # Why there was nothing here
        //
        // Every other condition on this screen is about one device. This is the
        // only one about the pair, and its absence was the largest hole in the
        // surface: §1.2 is explicit that replication is what protects against
        // losing a device, and a replication that quietly stopped is a person
        // believing they have two copies while they have one.
        //
        // The failure has no symptom. Captures land locally, every screen is
        // correct, nothing is slow. It is discovered when somebody goes looking
        // on the desktop for four days of notes that were never sent — which is
        // the "stops holding it in mind" failure at the scale of the archive.
        //
        // # Two questions, not one
        //
        // A round that ran and reached nobody and a round that never ran are
        // different situations with different fixes, and the peer rows report
        // them identically. So they are asked separately:
        //
        // - *Has a round run?* If not, sync is not happening — the periodic job
        //   is being held off, the application was force-stopped, the OEM put it
        //   to sleep. Nothing about the network is wrong.
        // - *Did it reach anyone?* If rounds run and reach nobody, the network
        //   is the problem: a VPN, a guest SSID, discovery unavailable, an
        //   address that changed.
        //
        // # Why the thresholds are where they are
        //
        // The period is fifteen minutes and Doze stretches it to hours, so
        // anything under a day is noise. A day of silence is still ordinary —
        // the other device was off. Three days is not: no plausible week has the
        // desktop closed for three days while the phone is in daily use, and by
        // then there is enough unreplicated work that losing the phone would
        // cost real notes.
        //
        // Reported with *when*, never as a bare "sync is broken". The date is
        // what tells somebody whether this started before or after the thing
        // they changed.
        // The state of sync, decided in `capture-store` so that this surface,
        // the window and the command line give one answer.
        //
        // The two questions this used to ask inline are now the order of
        // `SyncState`'s arms. Moving them was the point: they were asked
        // correctly *here* and not at all on the other two, which reported
        // "device X has never been reached" at somebody whose daemon had simply
        // never been started.
        let now = jiff::Timestamp::now().as_millisecond().max(0) as u64;
        // Membership from the log, and *not* from `peer_addresses`: a device
        // paired and never yet dialled has a roster entry and no row there,
        // which is the case this is most needed for.
        let roster_log = capture_store::log::Log::open_read(&self.paths.events)?;
        let me_key = capture_store::Identity::load(&self.paths)?
            .public()
            .to_bytes();
        let others = capture_core::roster::build(&roster_log.all()?)
            .keys
            .iter()
            .filter(|k| **k != me_key)
            .count();
        let sync = capture_store::syncstate::sync_state(&ops, others, now)?;
        if let Some((tier, text)) = sync.message() {
            out.push(Condition {
                tier: tier.into(),
                permission: None,
                text,
            });
        }

        if !matches!(sync, capture_store::syncstate::SyncState::Alone) {
            // §5.2's named trap, on a surface rather than in a toast.
            //
            // It was reported by `Toast.note` from a fifteen-minute background
            // job: three and a half seconds over whatever application happened
            // to be in front, possibly with the screen off, recorded nowhere.
            // The one message whose whole purpose is to prevent a silent
            // failure was itself effectively silent.
            if ops.last_round_had_discovery()? == Some(false) {
                out.push(Condition {
                    tier: "worth-knowing".into(),
                    permission: None,
                    text: "the last sync ran without discovery, so it could only \
                           reach devices whose address is already known"
                        .into(),
                });
            }

            // Said only when it is the reason something is not working. A peer
            // that refused one dial is one peer, and the round after it
            // succeeded — reporting that would be reporting the ordinary.
            //
            // "Not working" is `SyncState` rather than a second look at the
            // peer rows: anything that is not `Working` is a state where the
            // trouble string might be the explanation, and that is the same
            // judgement the condition above it just made.
            if let Some(t) = ops.last_round_trouble()?
                && !matches!(sync, capture_store::syncstate::SyncState::Working)
            {
                out.push(Condition {
                    tier: "worth-knowing".into(),
                    permission: None,
                    text: format!("the last sync reported: {t}"),
                });
            }
        }

        // A choice about where alerts land, when it lands nowhere.
        //
        // Now that the delivery paths actually honour this setting, a choice
        // that names no device this archive knows about is total silence on
        // every device — which is a worse failure than the duplicate alerts it
        // was set to stop, and one with no symptom other than nothing ever
        // happening again.
        if let Some(who) = capture_store::notifier::chosen(&store)? {
            let me = capture_store::Identity::load(&self.paths)?;
            let mine = capture_core::id::hexed(&me.public().to_bytes());
            let wanted = who.trim().to_ascii_lowercase();

            let log = capture_store::log::Log::open_read(&self.paths.events)?;
            let known = capture_core::roster::build(&log.all()?)
                .keys
                .iter()
                .any(|k| capture_core::id::hexed(k).starts_with(&wanted));

            if !known {
                out.push(Condition {
                    tier: "attention".into(),
                    permission: None,
                    text: format!(
                        "times are set to be announced on device {who}, which is not a \
                         device this archive knows — so nothing will be announced anywhere"
                    ),
                });
            } else if !mine.starts_with(&wanted) {
                out.push(Condition {
                    tier: "worth-knowing".into(),
                    permission: None,
                    text: format!("times are announced on device {who}, not on this one"),
                });
            }
        }

        for (_id, reason) in store.quarantined()? {
            out.push(Condition {
                tier: "worth-knowing".into(),
                permission: None,
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

    /// A list: the items written under a tag, then the tasks carrying it.
    ///
    /// Operation-shaped rather than type-shaped, like everything else here. The
    /// alternative was exposing a general "notes carrying a tag" read and a
    /// general "promote to task" write for Kotlin to compose into a list —
    /// which is Kotlin deciding what a list is, and `make android-decides`
    /// exists to stop exactly that.
    pub fn list(&self, tag: String) -> Result<Vec<ListLine>> {
        let store = self.read()?;
        Ok(store
            .list(&tag, self.now_ms())?
            .into_iter()
            .map(|e| {
                let slug = store.slug_of(&e.tgt).ok().flatten().unwrap_or_default();
                ListLine {
                    text: e.text,
                    done: e.done,
                    ord: e.ord,
                    slug,
                }
            })
            .collect())
    }

    /// Tick an item off a list, or put it back.
    pub fn tick(&self, tag: String, item: String, done: bool) -> Result<Ticked> {
        let _w = self.write_lock()?;
        Ok(
            match capture_store::promote::tick(&self.paths, &tag, &item, done, self.now_ms())? {
                capture_store::promote::Ticked::Done => Ticked::Done,
                capture_store::promote::Ticked::Already => Ticked::Already,
                capture_store::promote::Ticked::Ambiguous(n) => {
                    Ticked::Ambiguous { count: n as u32 }
                }
                capture_store::promote::Ticked::NoSuchItem => Ticked::NoSuchItem,
            },
        )
    }

    /// Tick an item of one capture, by where it sits in it.
    ///
    /// What a surface should use. A list on screen already knows which row was
    /// touched, so there is nothing to match by words — which is the whole of
    /// why two items reading the same were untickable before this: `tick`
    /// found two, could not tell them apart, and refused. "milk, milk" is an
    /// ordinary thing to write when two shops are meant.
    ///
    /// The position is recorded with the tick, so correcting a word later does
    /// not discard it.
    pub fn tick_at(&self, name: String, ord: u32, done: bool) -> Result<Ticked> {
        let _w = self.write_lock()?;
        Ok(
            match capture_store::promote::tick_at(&self.paths, &name, ord, done)? {
                capture_store::promote::Ticked::Done => Ticked::Done,
                capture_store::promote::Ticked::Already => Ticked::Already,
                capture_store::promote::Ticked::Ambiguous(n) => {
                    Ticked::Ambiguous { count: n as u32 }
                }
                capture_store::promote::Ticked::NoSuchItem => Ticked::NoSuchItem,
            },
        )
    }

    /// Mark a task done, or not.
    pub fn set_done(&self, name: String, done: bool) -> Result<()> {
        let _w = self.write_lock()?;
        Ok(capture_store::promote::set_done(&self.paths, &name, done)?)
    }

    /// Say what a capture is.
    ///
    /// A person deciding, so the promotion is pinned and no later reparse
    /// displaces it — the one conflict rule here that is not last-write-wins,
    /// because a grammar improvement silently reverting somebody's correction
    /// is the failure §6 rates unrecoverable.
    ///
    /// Demotion to a plain note goes through the same call. A thing wrongly
    /// promoted is as ordinary as a thing wrongly left alone, and making one of
    /// those cheap and the other a trip to a command line is the gradient §8.4
    /// says to avoid.
    ///
    /// The captured text is not touched. Promotion annotates; it never
    /// rewrites (§3.1.3).
    pub fn set_kind(&self, name: String, kind: NoteKind) -> Result<()> {
        let _w = self.write_lock()?;
        let as_ = match kind {
            NoteKind::Note => capture_core::body::Promoted::Note,
            // No due date on this path. Saying "this is a task" and saying when
            // it is due are separate decisions, and `as_task` is the one that
            // takes a day.
            NoteKind::Task => capture_core::body::Promoted::Task(capture_core::body::TaskFields {
                done: false,
                due: None,
                order: None,
            }),
        };
        Ok(capture_store::promote::correct(&self.paths, &name, as_)?)
    }

    /// What the archive thinks this note might be, where it was unsure enough
    /// not to have acted.
    ///
    /// A call of its own rather than a field on `NoteSummary`, because filling
    /// it there would mean running the recognizer once per timeline row to
    /// answer a question only the note page asks. The alternative — a field
    /// filled by one read and left empty by another — is the divergence that
    /// putting the descriptor in the projection was meant to end.
    ///
    /// `None` where the reading is confident enough to have been applied, and
    /// where there is nothing temporal in the text at all.
    pub fn suggestion(&self, name: String) -> Result<Option<String>> {
        let store = self.read()?;
        let tgt = store
            .resolve(&name)?
            .ok_or_else(|| CaptureError::NotFound { name: name.clone() })?;
        let Some(row) = store.note(&tgt)? else {
            return Err(CaptureError::NotFound { name });
        };
        let Some(text) = row.text.as_deref() else {
            return Ok(None);
        };
        // Against when it was captured, as every other reading of this text is.
        Ok(capture_store::review::suggestion_for(
            text,
            row.tz.as_deref(),
            row.created.0 as i64,
        ))
    }

    /// Insert the character a capture was missing to be a list.
    ///
    /// An ordinary edit, authored because somebody asked. Different in kind from
    /// `accept`, which annotates and leaves the words alone — this rewrites
    /// them, which is why a surface offering it says what will be inserted
    /// rather than offering a bare yes.
    ///
    /// Answers the text as it now reads.
    pub fn accept_near_miss(&self, name: String) -> Result<String> {
        let _w = self.write_lock()?;
        Ok(capture_store::mutate::accept_near_miss(&self.paths, &name)?)
    }

    /// Take the reading the archive offered but would not act on.
    ///
    /// The other half of the confidence floor. Below it a recognition is shown
    /// and applied to nothing, which is the right answer for a machine deciding
    /// by itself and the wrong one as a final answer — the person reading the
    /// suggestion wrote the text, and knows whether it meant Tuesday.
    ///
    /// Not the same as lowering the floor. The floor governs what is written
    /// unasked; this is somebody saying yes. The result is pinned, so no later
    /// grammar improvement undoes it.
    ///
    /// Answers what it became: `note`, `task`, `event` or `series`.
    pub fn accept(&self, name: String) -> Result<String> {
        let _w = self.write_lock()?;
        let as_ = capture_store::promote::accept(&self.paths, &name)?;
        Ok(String::from(match as_ {
            capture_core::body::Promoted::Note => "note",
            capture_core::body::Promoted::Task(_) => "task",
            capture_core::body::Promoted::Event(_) => "event",
            capture_core::body::Promoted::Series(_) => "series",
        }))
    }

    /// Make something a task, with a due date or without one.
    ///
    /// The due date is a floating day — `2026-09-20` — and never an instant.
    /// "Due Friday" is not 00:00 UTC on Friday (§3.6), and a malformed date is
    /// refused here rather than stored and later found by a query that quietly
    /// matches nothing.
    pub fn as_task(&self, name: String, due: Option<String>) -> Result<()> {
        let _w = self.write_lock()?;
        Ok(capture_store::promote::as_task(
            &self.paths,
            &name,
            due.as_deref(),
        )?)
    }

    /// A note's text, split into ordinary runs and mentions.
    ///
    /// Given the text rather than a note name, because the caller already has it
    /// and a second read to fetch it again could disagree with what is on screen.
    ///
    /// A mention that cannot be found again in the text is left as ordinary
    /// words, which is the safe direction: the reader sees exactly what was
    /// written, and the worst case is a tap that is not offered.
    pub fn pieces(&self, text: String) -> Vec<TextPiece> {
        let found = capture_parse::mentions(&text);
        if found.is_empty() {
            return vec![TextPiece {
                text,
                mention: None,
            }];
        }

        let mut out = Vec::new();
        let mut rest = text.as_str();
        for (sigil, value) in found {
            let whole = format!("{sigil}{value}");
            let Some(at) = rest.find(&whole) else {
                continue;
            };
            if at > 0 {
                out.push(TextPiece {
                    text: rest[..at].to_string(),
                    mention: None,
                });
            }
            out.push(TextPiece {
                text: whole.clone(),
                mention: Some(whole.clone()),
            });
            rest = &rest[at + whole.len()..];
        }
        if !rest.is_empty() {
            out.push(TextPiece {
                text: rest.to_string(),
                mention: None,
            });
        }
        out
    }

    /// Everyone the archive knows.
    ///
    /// An entity exists because a note named it, so this needs nothing created
    /// and is empty only when nothing has been named (§3.7).
    pub fn people(&self) -> Result<Vec<Entity>> {
        self.entities_of("person")
    }

    /// Every project it knows.
    pub fn projects(&self) -> Result<Vec<Entity>> {
        self.entities_of("project")
    }

    /// One entity's page, by a mention as it is written: `@sarah`, `^acme`.
    ///
    /// The sigil is read here rather than by the caller. A phone deciding that
    /// `@` means a person is a phone deciding something the store owns — the
    /// same reason `lists` refused to expose a general `mentioning`, and what
    /// `make android-decides` exists to catch.
    ///
    /// `None` where no note names it, which is an ordinary answer.
    pub fn entity(&self, mention: String) -> Result<Option<Entity>> {
        let (kind, value) = split_mention(&mention)?;
        let store = self.read()?;
        let Some(row) = store.entity(kind, &value)? else {
            return Ok(None);
        };
        Ok(Some(self.entity_page(&store, row)?))
    }

    /// Give an entity a name of your choosing.
    ///
    /// Pinned, like every correction: no later reparse displaces it, and the id
    /// it is written against does not change, so the page does not fork.
    pub fn rename_entity(&self, mention: String, display: String) -> Result<()> {
        let _w = self.write_lock()?;
        let (kind, value) = split_mention(&mention)?;
        Ok(capture_store::mutate::rename_entity(
            &self.paths,
            kind,
            &value,
            &display,
        )?)
    }

    /// Say that two entities were one all along.
    ///
    /// A redirection: nothing is moved, no capture changes, and either spelling
    /// then reaches the same page. Refused across kinds — a person and a project
    /// sharing a word is the ordinary case, not a mistake.
    pub fn merge_entities(&self, from: String, into: String) -> Result<()> {
        let _w = self.write_lock()?;
        let (into_kind, into_value) = split_mention(&into)?;
        // The one being merged *in* may be written bare. It is being joined to
        // an entity whose kind is already known, so demanding the sigil again
        // asks somebody to restate what the surface they are looking at already
        // says — and getting it wrong is then their error rather than the
        // question's. Driving this on a phone is what surfaced it.
        let (from_kind, from_value) = split_mention_or(&from, into_kind)?;
        if from_kind != into_kind {
            return Err(CaptureError::Refused {
                reason: format!(
                    "{from} and {into} are not the same kind of thing; \
                     a {from_kind} and a {into_kind} stay separate"
                ),
            });
        }
        Ok(capture_store::mutate::merge_entities(
            &self.paths,
            from_kind,
            &from_value,
            &into_value,
        )?)
    }

    /// Bind the note that *is* an entity, or unbind it with `None`.
    ///
    /// The note keeps being whatever it was. §4.3 puts this on the entity rather
    /// than on the note so that what a note can be stays `note | task | event |
    /// series` — a backing note may be a task, and goes on appearing wherever
    /// tasks appear.
    pub fn bind_backing(&self, mention: String, note: Option<String>) -> Result<()> {
        let _w = self.write_lock()?;
        let (kind, value) = split_mention(&mention)?;
        Ok(capture_store::mutate::bind_backing(
            &self.paths,
            kind,
            &value,
            note.as_deref(),
        )?)
    }

    /// Every open task, banded into overdue, due and undated.
    ///
    /// Today is worked out here rather than taken from the caller, for the
    /// reason `agenda` gives: what day it is in the reading zone is a decision
    /// with one implementation, and a surface passing its own answer is how
    /// `today()` and `agenda()` came to disagree about the date.
    ///
    /// Finished tasks are absent. Nothing counts them.
    pub fn tasks(&self) -> Result<Vec<TaskLine>> {
        let store = self.read()?;
        let today = self.today();
        let mut out = Vec::new();
        for row in store.tasks_outstanding(&today)? {
            out.push(TaskLine {
                slug: store.slug_of(&row.tgt)?.unwrap_or_default(),
                text: store.text_of(&row.tgt)?,
                descriptor: row.descriptor,
                due: row.due,
                band: match row.band {
                    capture_store::projection::Band::Overdue => TaskBand::Overdue,
                    capture_store::projection::Band::Due => TaskBand::Due,
                    capture_store::projection::Band::Undated => TaskBand::Undated,
                },
            });
        }
        Ok(out)
    }

    /// Stop accepting what a device writes from here on.
    ///
    /// The remedy for a device that was lost or stolen. It retires the *key*,
    /// not an origin: retiring an origin would let the device mint a fresh
    /// incarnation on its next restart and walk straight back in.
    ///
    /// Nothing is deleted. A retired device's past events were legitimately
    /// authored and stay valid — discarding them would mean losing real notes,
    /// which is a worse outcome than the access this closes.
    ///
    /// Here because the phone may be the only device left. Retiring a lost
    /// phone from the desktop was always possible; a lost *desktop* left the
    /// phone holding the archive and unable to close anything, which is the
    /// half of §5.3's remedy that was never reachable.
    ///
    /// The key is matched by prefix, as it is shown in `peers`. An ambiguous
    /// prefix is refused rather than resolved — retiring the wrong device is
    /// undone only by the two-signature readmission below.
    pub fn retire(&self, key: String) -> Result<String> {
        let _w = self.write_lock()?;

        let identity = capture_store::identity::Identity::load(&self.paths)?;
        let log = capture_store::log::Log::open_read(&self.paths.events)?;
        let roster = capture_core::roster::build(&log.all()?);
        drop(log);

        let wanted = key.to_ascii_lowercase();
        let matches: Vec<[u8; 32]> = roster
            .keys
            .iter()
            .copied()
            .filter(|k| short_key(k).starts_with(&wanted))
            .collect();

        let subject = match matches.as_slice() {
            [] => {
                return Err(CaptureError::NotFound {
                    name: format!("no device in the roster matches {key}"),
                });
            }
            [one] => *one,
            many => {
                return Err(CaptureError::Refused {
                    reason: format!("{} devices start with {key} — give more of it", many.len()),
                });
            }
        };

        // A roster with nobody in it can never admit anyone again, and there
        // is no way back from that: readmission needs a signature from a
        // member, and there would be none.
        if roster.keys.len() == 1 {
            return Err(CaptureError::Refused {
                reason: "this is the only device in the roster, and a roster with nobody in \
                         it can never admit anyone again"
                    .into(),
            });
        }

        let _ = identity;
        let mut s = capture_store::session::Session::open(self.paths.clone())?;
        s.retire(&subject, None)?;
        Ok(short_key(&subject))
    }

    /// What this device needs another to vouch for, to get back in.
    ///
    /// Shown on the device that has been locked out. It is not a secret: it
    /// names which device is asking and nothing else, and a signature over it
    /// is useless without the second one from the device it names.
    pub fn readmission_request(&self) -> Result<String> {
        let identity = capture_store::identity::Identity::load(&self.paths)?;
        Ok(format!(
            "{}.{}",
            hexed(&identity.public().to_bytes()),
            hexed(&identity.incarnation().0),
        ))
    }

    /// Vouch for another device's re-admission.
    ///
    /// Run on a device that still works, with the request the locked-out one
    /// showed. §5.3 needs two signatures because any member may retire any
    /// member: one compromised device can retire every other, after which
    /// `device_add` cannot help — it needs a signature from a current member
    /// and there are none left.
    ///
    /// Where this device is still a member it writes the re-admission itself
    /// rather than handing back a signature to carry. That is not a
    /// convenience: a retired device cannot open a session, so a re-admission
    /// it authored has no way to reach the devices refusing it, and the remedy
    /// cannot travel through the refusal it exists to undo.
    pub fn cosign(&self, request: String) -> Result<String> {
        let _w = self.write_lock()?;
        let identity = capture_store::identity::Identity::load(&self.paths)?;

        let (key, incarnation) = request
            .split_once('.')
            .ok_or_else(|| CaptureError::Refused {
                reason: "that is not a re-admission request".into(),
            })?;
        let (Some(key), Some(incarnation)) = (unhex(key), unhex(incarnation)) else {
            return Err(CaptureError::Refused {
                reason: "that is not a re-admission request".into(),
            });
        };
        let (Ok(key), Ok(incarnation)) = (
            <[u8; 32]>::try_from(key.as_slice()),
            <[u8; 16]>::try_from(incarnation.as_slice()),
        ) else {
            return Err(CaptureError::Refused {
                reason: "that is not a re-admission request".into(),
            });
        };

        if key == identity.public().to_bytes() {
            return Err(CaptureError::Refused {
                reason: "a device cannot vouch for itself — that is one device's say-so \
                         written twice"
                    .into(),
            });
        }

        // A fixed nonce, so the two devices do not have to agree one first.
        // The digest is domain-separated and says only which device is meant,
        // so a second copy of the signature re-admits the same device, which
        // is what it already says.
        let nonce = [0u8; 16];
        let digest = capture_core::body::readmit_digest(&key, &incarnation, &nonce);
        let sig = ed25519_dalek::Signer::sign(identity.signing(), &digest);

        let log = capture_store::log::Log::open_read(&self.paths.events)?;
        let mut roster = capture_core::roster::build(&log.all()?);
        roster.honour_self(&identity.public().to_bytes());
        drop(log);

        if roster.keys.contains(&identity.public().to_bytes()) {
            let subject = capture_store::session::Admitted {
                pubkey: key,
                incarnation,
                // Not known here and not needed: an origin claim or a
                // pairing publishes it, and this device already holds one for
                // a peer it had admitted before. Zero would be a claim, so the
                // roster's own record is left to stand.
                noise: [0u8; 32],
            };
            let mut s = capture_store::session::Session::open(self.paths.clone())?;
            s.readmit_with(
                &subject,
                &identity.public().to_bytes(),
                &sig.to_bytes(),
                nonce,
            )?;
            return Ok(String::new());
        }

        // Not a member either. All this device can do is hand back a
        // signature for somebody to carry.
        Ok(format!(
            "{}.{}",
            hexed(&sig.to_bytes()),
            hexed(&identity.public().to_bytes()),
        ))
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

    /// Capture a recording (§5.6, §2.2).
    ///
    /// The bytes are stored before the event is written, so a note can never
    /// refer to a recording nobody holds. It is an ordering rather than a
    /// check, which makes the bad state impossible instead of unlikely.
    ///
    /// No text, and no transcription here. §2.2 says the phone's job ends at
    /// "the audio exists" — a transcript is enrichment, it arrives later as an
    /// edit if it arrives at all, and §4.3 forbids deriving the note's
    /// permanent name from it because machine output changes and a name must
    /// not.
    ///
    /// Returns the slug, which is derived from the date because there are no
    /// words yet.
    pub fn capture_voice(
        &self,
        bytes: Vec<u8>,
        media_type: String,
        duration_ms: Option<u32>,
    ) -> Result<String> {
        let _writing = self.write_lock()?;

        let hash = capture_store::blobs::put(&self.paths.blobs, &bytes)?;
        let body = capture_store::session::capture_body_with_blob(
            "",
            capture_core::body::BlobRef {
                hash,
                media_type,
                bytes: bytes.len() as u64,
                duration_ms,
            },
        )?;

        match capture_store::session::commit_body(&self.paths, &body)? {
            Committed::Stored(slug) => Ok(slug),
            // A recording that could not be committed is not spooled: the
            // spool holds text, and forty megabytes of audio in it would be a
            // different mechanism wearing the same name. The bytes are on
            // disk and the event is not, which is recoverable and is said
            // plainly rather than reported as success.
            Committed::Spooled => Err(CaptureError::Refused {
                reason: "the store could not be written, so the recording was kept \
                         but no note was made for it"
                    .into(),
            }),
        }
    }

    /// The recording a note refers to, if it refers to one.
    ///
    /// Read from the event rather than the projection, which has no blob table
    /// yet. One note, one lookup — a timeline does not ask this, so the cost
    /// stays where a person opened something.
    pub fn recording_of(&self, name: String) -> Result<Option<Recording>> {
        let store = self.read()?;
        let tgt = store
            .resolve(&name)?
            .ok_or_else(|| CaptureError::NotFound { name: name.clone() })?;

        let log = capture_store::log::Log::open_read(&self.paths.events)?;
        let Some(event) = log.by_id(&tgt)? else {
            return Ok(None);
        };
        let Some(body) = event.body.as_deref() else {
            // Redacted. The reference went with the body, which is the point:
            // a destroyed note does not leave a surface offering to play what
            // it was destroyed to be rid of.
            return Ok(None);
        };
        let Ok(capture) = minicbor::decode::<capture_core::body::CaptureBody>(body) else {
            return Ok(None);
        };

        let Some(b) = capture.blob else {
            return Ok(None);
        };
        let name = capture_store::blobs::name(&b.hash);
        let here = capture_store::blobs::have(&self.paths.blobs, &b.hash);
        let now = self.now_ms();
        let unavailable_for_days = if here {
            None
        } else {
            capture_store::ops::Ops::open_read(&self.paths.ops)
                .and_then(|ops| ops.blob_unavailable_since(&name, now))
                .unwrap_or(None)
                .map(|since| (now.saturating_sub(since) / 86_400_000) as u32)
        };
        Ok(Some(Recording {
            name,
            media_type: b.media_type,
            bytes: b.bytes,
            duration_ms: b.duration_ms,
            here,
            unavailable_for_days,
        }))
    }

    /// The bytes of a recording, if this device holds them.
    ///
    /// `None` where it does not, which is the ordinary state of a phone that
    /// evicts aggressively (§5.6) rather than a fault — the bytes come back on
    /// the next round with a device that has them.
    pub fn recording(&self, name: String) -> Result<Option<Vec<u8>>> {
        let Some(hash) = capture_store::blobs::parse(&name) else {
            return Err(CaptureError::NotFound { name });
        };
        Ok(capture_store::blobs::get(&self.paths.blobs, &hash)?)
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
        // Not marked here. The caller has to post a notification, and on
        // Android that can be accepted and dropped — a muted channel, or the
        // permission revoked in Settings — with no error anywhere.
        //
        // Marking first meant an arrival was spent on something nobody saw,
        // and a place cue cannot be retried: `may_fire_again` holds it for
        // half an hour, and standing inside the fence produces no new entry
        // transition. The visit is simply gone.
        //
        // This is the same failure that was fixed on the time path, where
        // `notify` and `say` were made to report whether they spoke. The
        // location path never got it.
        Ok(Some(intent.then_do))
    }

    /// Say that a cue was actually delivered.
    ///
    /// Separate from [`Self::arrived`] so that a cue is spent only when it was
    /// heard. The caller knows whether the platform took the notification;
    /// this crate cannot.
    pub fn cue_delivered(&self, id: String) -> Result<()> {
        let Some(raw) = unhex(&id) else {
            return Err(CaptureError::NotFound { name: id });
        };
        let now = jiff::Timestamp::now().as_millisecond().max(0) as u64;
        capture_store::ops::Ops::open(&self.paths.ops)?.mark_intent_fired(&raw, now)?;
        Ok(())
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

    /// Whether this device is the one that says things (§2.4.1).
    ///
    /// Asked by the alarm path before it arms anything. The setting had a
    /// getter and a setter and no reader on any delivery path, so choosing a
    /// notifier changed nothing and both paired devices announced everything.
    ///
    /// When nobody has chosen, §2.4.1's default rule decides — the most
    /// recently paired device that can notify — rather than every device
    /// answering yes, which is what this did and why a paired phone and
    /// desktop both announced everything.
    pub fn says_things_here(&self) -> Result<bool> {
        let me = capture_store::Identity::load(&self.paths)?;
        Ok(capture_store::notifier::speaks_here(
            &self.paths,
            &self.read()?,
            &me.public().to_bytes(),
            self.now_ms(),
        )?)
    }

    /// The device the default rule picks, by its short key, so a screen can
    /// say who speaks when nobody has chosen rather than leave it implied.
    pub fn default_notifier(&self) -> Result<String> {
        let me = capture_store::Identity::load(&self.paths)?;
        let key = capture_store::notifier::default_here(
            &self.paths,
            &me.public().to_bytes(),
            self.now_ms(),
        )?;
        Ok(capture_core::id::short_key(&key))
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

    /// Say how long a capture took, gesture to committed.
    ///
    /// Reported by the surface because only the surface knows when the gesture
    /// began — the archive is handed text that already exists. This is
    /// measuring, not deciding: the number is a fact about how long this
    /// device took, and what to do about it is nobody's decision to make here.
    ///
    /// §2.1 argues capture must cost almost nothing at the moment a thought
    /// arrives. A system that asserts that without measuring it is making a
    /// promise it has no way to keep, and 4.5 asks the phone to answer the
    /// same question the desktop's chord already answers.
    pub fn took(&self, surface: String, ms: u64) -> Result<()> {
        let ops = capture_store::ops::Ops::open(&self.paths.ops)?;
        ops.record_capture_latency(&surface, ms)?;
        Ok(())
    }

    /// What each capture surface has cost, as a sentence per surface.
    pub fn costs(&self) -> Result<Vec<String>> {
        let ops = capture_store::ops::Ops::open_read(&self.paths.ops)?;
        let mut out = Vec::new();
        for surface in ops.measured_surfaces()? {
            if let Some(summary) = ops.capture_latency(&surface)?.summary() {
                out.push(format!("{surface}: {summary}"));
            }
        }
        Ok(out)
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
    ///
    /// The promotions happen inside `Session::open`, which sweeps on the way
    /// in — so this is also what reads a capture made since the app was last
    /// open. That is why there is no daemon-shaped hole on this platform and
    /// there was one on the desktop, where nothing owned the sweep at all.
    ///
    /// The returned count is the *due work* drained, not the captures read. A
    /// surface asking whether it is looking at stale state wants
    /// [`Archive::unread`], which answers without taking the writer lock.
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
        Ok(capture_core::id::short_key(&identity.public().to_bytes()))
    }
}

impl Archive {
    /// Serialise writes from this process.
    fn write_lock(&self) -> Result<std::sync::MutexGuard<'_, ()>> {
        self.writing.lock().map_err(|_| CaptureError::Store {
            reason: "a previous write left the archive locked".into(),
        })
    }

    /// The wall clock, read at the binding's edge.
    ///
    /// This is where a clock is allowed: `capture-store` is kept pure so that
    /// folding the same events gives the same answer everywhere, which means
    /// the surfaces supply the one thing that genuinely varies — what time it
    /// is for the person holding the phone.
    fn now_ms(&self) -> u64 {
        #[allow(
            clippy::disallowed_methods,
            clippy::disallowed_types,
            reason = "the binding is the edge where the wall clock is read"
        )]
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Every entity of a kind, as pages.
    fn entities_of(&self, kind: &str) -> Result<Vec<Entity>> {
        let store = self.read()?;
        let now = jiff::Timestamp::now().as_millisecond().max(0) as u64;
        let mut out = Vec::new();
        for row in store.entities_of_kind(kind, now)? {
            out.push(self.entity_page(&store, row)?);
        }
        Ok(out)
    }

    /// Fill in a page from its row: the backing note, then the notes naming it.
    fn entity_page(
        &self,
        store: &capture_store::projection::Store,
        row: capture_store::projection::EntityRow,
    ) -> Result<Entity> {
        let now = jiff::Timestamp::now().as_millisecond().max(0) as u64;
        let backing = match row.backing {
            Some(tgt) => store.note(&tgt)?.map(summarise),
            None => None,
        };
        let notes = store
            .entity_notes(&row.ent, now)?
            .into_iter()
            .map(summarise)
            .collect();
        Ok(Entity {
            display: row.display,
            kind: row.kind,
            names: row.names,
            backing,
            notes,
        })
    }

    /// The projection, without taking the writer lock.
    ///
    /// Reads never block on a write and never block a write. `capture verify`
    /// exists for the moment the archive is unhealthy, and a diagnostic that
    /// stops working when the system is unwell is one you cannot use when you
    /// need it.
    fn read(&self) -> Result<capture_store::projection::Store> {
        Ok(capture_store::projection::current(&self.paths)?)
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
        Ok(capture_core::id::short_key(&joined))
    }
}

/// Where the location permission is remembered.
const LOCATION_PERMITTED: &str = "android.location_permitted";

/// Where the platform's own idea of this device's zone is remembered.
const DEVICE_ZONE: &str = "device.zone";

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
    capture_core::id::unhex(s)
}

fn named(key: &[u8; 32]) -> String {
    capture_core::id::short_key(key)
}

/// A moment, in this device's zone.
fn when(ms: u64) -> String {
    let here = jiff::tz::TimeZone::system();
    let (day, at) = rendered(ms as i64, here.iana_name());
    format!("{day} {at}")
}

/// A summary, completed from the projection it was read out of.
///
/// One function rather than one per call site. The timeline and the note page
/// are separate queries, and the whole argument for keeping the rendering in
/// the projection is that two readings of one note cannot disagree — which
/// they would the first time somebody fixed one call site and not the other.
fn read_as(
    store: &capture_store::projection::Store,
    n: capture_store::projection::NoteRow,
) -> Result<NoteSummary> {
    let tgt = n.tgt;
    let tag = store.list_tag_of(&tgt)?;
    let mut s = summarise(n);

    if let Some(p) = store.promotion_of(&tgt)? {
        s.kind = p.kind;
        s.descriptor = p.descriptor;
    }

    // Only a list has items, and asking for them costs a query — so it is
    // asked only where the fold already said there is a list to ask about.
    if tag.is_some() {
        // This capture's own items, by one indexed query. Asking `list` and
        // filtering would gather the whole tag — every capture under it and
        // every task carrying it — once per row, which measured at 66ms for a
        // hundred rows.
        s.items = store
            .items_of(&tgt)?
            .into_iter()
            .map(|e| ListLine {
                text: e.text,
                done: e.done,
                ord: e.ord,
                // Its own items, so the capture is the one being summarised.
                slug: s.slug.clone(),
            })
            .collect();
    }
    s.list_tag = tag;
    Ok(s)
}

/// A key as the surfaces show it.
///
/// Delegates, because how much of a key a person is shown is one decision
/// (`capture_core::id::SHOWN_KEY_BYTES`) and this file used to make it three
/// times on its own.
fn short_key(k: &[u8; 32]) -> String {
    capture_core::id::short_key(k)
}

fn hexed(b: &[u8]) -> String {
    capture_core::id::hexed(b)
}

/// Split `@sarah` into the kind it names and the name itself.
///
/// In Rust rather than in Kotlin, so that what `@` means is decided once and in
/// the place that owns it.
fn split_mention(mention: &str) -> Result<(&'static str, String)> {
    split_mention_or(mention, "")
}

/// The same, accepting a bare name where the kind is already known.
///
/// `default_kind` empty means a sigil is required.
fn split_mention_or(mention: &str, default_kind: &'static str) -> Result<(&'static str, String)> {
    let mut chars = mention.chars();
    let sigil = chars.next().unwrap_or(' ');
    let value: String = chars.collect();
    let (kind, value) = match capture_core::id::entity_kind(sigil) {
        Some(kind) => (kind, value),
        // A bare name, where the caller already knows what it is naming — the
        // entity screens ask about a person they are already showing.
        None if !default_kind.is_empty() => (default_kind, mention.to_string()),
        None => {
            return Err(CaptureError::Refused {
                reason: format!("'{mention}' is not a mention; use @person, ^project or #tag"),
            });
        }
    };
    if value.is_empty() {
        return Err(CaptureError::Refused {
            reason: format!("'{mention}' names nothing"),
        });
    }
    Ok((kind, value))
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
        // Filled by the caller, which has the store open. Left absent here
        // rather than guessed at: a summary that re-parsed the text would be a
        // second implementation of the list form.
        list_tag: None,
        kind: "note".into(),
        descriptor: String::new(),
        items: Vec::new(),
    }
}

/// A zone by name, or UTC.
///
/// Outside the exported block: everything in there crosses the boundary, and a
/// zone is not something Kotlin is handed.
fn resolve_zone(name: &str) -> Result<capture_time::Zone> {
    capture_time::Zone::new(name)
        .or_else(|| capture_time::Zone::new("UTC"))
        .ok_or_else(|| CaptureError::Refused {
            reason: "no usable timezone".into(),
        })
}

/// The next instant a given local hour comes round.
///
/// Today if that hour is still ahead, tomorrow otherwise. Equal counts as past:
/// it is already that hour, and the decision to hold was made knowing so.
fn at_hour(now: i64, zone: capture_time::Zone, hour: u32) -> Result<i64> {
    let today = capture_time::civil_date(now, zone);
    let date = if hour > capture_time::civil_clock(now, zone).0 {
        today
    } else {
        capture_time::add_days(now, zone, 1).ok_or_else(|| CaptureError::Refused {
            reason: "tomorrow could not be worked out".into(),
        })?
    };
    capture_time::at_local(date, hour, 0, zone).ok_or_else(|| CaptureError::Refused {
        reason: "that hour does not exist today".into(),
    })
}

/// The decision behind [`Archive::next_wake`], with the clock passed in.
fn wake_for(
    alerts: &[Alert],
    now: i64,
    zone: capture_time::Zone,
    soonest_ms: i64,
) -> Result<Option<Wake>> {
    use capture_store::notifier::{Delivery, deliver_late};

    let hour = capture_time::civil_clock(now, zone).0;
    let mut best: Option<Wake> = None;
    for a in alerts.iter().filter(|a| !a.delivered) {
        let late_by = now - a.at_ms;
        let wake = if late_by < 0 {
            Wake {
                at_ms: a.at_ms,
                exact: a.timed,
            }
        } else {
            // `armed: false`, as the ring asks it. Asking differently here
            // would be arming for an answer delivery will not give.
            match deliver_late(hour, a.timed, false, late_by) {
                Delivery::Now => Wake {
                    at_ms: now + soonest_ms.max(1_000),
                    exact: false,
                },
                Delivery::At(h) => Wake {
                    at_ms: at_hour(now, zone, h)?,
                    exact: false,
                },
                Delivery::Stale => continue,
            }
        };
        best = match best {
            Some(b) if (b.at_ms, !b.exact) <= (wake.at_ms, !wake.exact) => Some(b),
            _ => Some(wake),
        };
    }
    Ok(best)
}

#[cfg(test)]
mod wake {
    use super::*;

    const HOUR: i64 = 60 * 60 * 1000;

    fn chicago() -> capture_time::Zone {
        capture_time::Zone::new("America/Chicago").unwrap()
    }

    /// An instant in Chicago, from a date and an hour.
    fn at(date: &str, hour: u32) -> i64 {
        capture_time::at_local(
            capture_time::FloatingDate::parse(date).unwrap(),
            hour,
            0,
            chicago(),
        )
        .unwrap()
    }

    fn alert(at_ms: i64, timed: bool, delivered: bool) -> Alert {
        Alert {
            slug: "n".into(),
            text: "t".into(),
            at_ms,
            occ: None,
            timed,
            delivered,
        }
    }

    #[test]
    fn a_moment_too_old_to_say_never_wakes_anything() {
        // The phone that found this: one invoice due a fortnight before,
        // undelivered on purpose, armed for a second from now for ever.
        let now = at("2026-10-10", 20);
        let old = alert(at("2026-09-20", 0), false, false);
        assert_eq!(wake_for(&[old], now, chicago(), 1_000).unwrap(), None);
    }

    #[test]
    fn a_held_moment_wakes_when_the_hold_ends_not_now() {
        // 23:00 is quiet. A capture due at 22:30 with no time of day is held
        // to morning, and the alarm belongs at the end of the hold.
        let now = at("2026-10-10", 23);
        let held = alert(now - HOUR / 2, false, false);
        let w = wake_for(&[held], now, chicago(), 1_000).unwrap().unwrap();
        assert!(w.at_ms > now + HOUR, "woke during the hold: {w:?}");
        assert_eq!(
            capture_time::civil_date(w.at_ms, chicago()).to_string(),
            "2026-10-11"
        );
        assert!(!w.exact);
    }

    #[test]
    fn something_ahead_wakes_at_its_own_moment() {
        let now = at("2026-10-10", 20);
        let ahead = alert(now + HOUR, true, false);
        let w = wake_for(&[ahead], now, chicago(), 1_000).unwrap().unwrap();
        assert_eq!(
            w,
            Wake {
                at_ms: now + HOUR,
                exact: true
            }
        );
    }

    #[test]
    fn something_due_now_waits_the_soonest_it_is_given() {
        let now = at("2026-10-10", 15);
        let due = || alert(now - 60_000, true, false);
        assert_eq!(
            wake_for(&[due()], now, chicago(), 1_000)
                .unwrap()
                .unwrap()
                .at_ms,
            now + 1_000
        );
        // After a ring that could not post it: minutes, not a loop.
        assert_eq!(
            wake_for(&[due()], now, chicago(), 5 * 60_000)
                .unwrap()
                .unwrap()
                .at_ms,
            now + 5 * 60_000
        );
    }

    #[test]
    fn the_earliest_of_several_wins_and_delivered_ones_are_ignored() {
        let now = at("2026-10-10", 20);
        let alerts = [
            alert(at("2026-09-20", 0), false, false),
            alert(now + 2 * HOUR, true, false),
            alert(now + HOUR, true, true),
            alert(now + 3 * HOUR, false, false),
        ];
        let w = wake_for(&alerts, now, chicago(), 1_000).unwrap().unwrap();
        assert_eq!(w.at_ms, now + 2 * HOUR);
    }
}
