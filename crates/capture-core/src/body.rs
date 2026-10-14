//! Event bodies. **Frozen per kind.**
//!
//! A body is hashed into `bhash`, which is hashed into `id`, which is signed.
//! So a body shape is permanent from its first use, and [`ENVELOPE_VERSION`]
//! does **not** cover it — a breaking change to a body requires a *new kind*,
//! not a version bump.
//!
//! [`ENVELOPE_VERSION`]: crate::ENVELOPE_VERSION
//!
//! # Field 0 is always the nonce
//!
//! Not "the first field" — with indexed encoding you do not control physical
//! position, so the requirement is stated as an index. See [`crate::Nonce`] for
//! why it exists at all.

use alloc::string::String;
use alloc::vec::Vec;

use minicbor::{Decode, Encode};

use crate::{
    Dev, Nonce,
    id::{EntId, EventId},
};

/// Raw captured input. Never rejected, never validated.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct CaptureBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt for `bhash`. Always present.
    pub nonce: [u8; 16],
    /// Exactly what was typed or spoken. Preserved verbatim, forever — never
    /// rewritten, reformatted, consumed or relocated. This is what makes a
    /// wrong interpretation survivable: a misparse becomes a wrong label on an
    /// intact note rather than a lost note.
    #[n(1)]
    pub text: String,
    /// The IANA zone the capture happened in.
    ///
    /// Lives in the body rather than being derived later because it cannot be
    /// recovered afterwards, and day grouping depends on it — a note captured at
    /// 23:40 in one zone must not file under a different day when read in
    /// another.
    #[n(2)]
    pub tz: Option<String>,
    /// Ambient context observed at capture, at no cost to the user.
    #[n(3)]
    pub ambient: Ambient,
    /// Bytes captured alongside — a recording, a photograph (§5.6).
    ///
    /// On the capture rather than in an event of its own, because the slug is
    /// derived from the root capture and nothing else (§4.3). A reference that
    /// arrived as a later event would make a note's permanent name depend on
    /// whether that event had turned up yet, and two replicas folding in
    /// different orders would name the same note differently.
    ///
    /// A trailing field, so every capture written before blobs existed encodes
    /// exactly as it did — minicbor elides a trailing `None`, so the id of
    /// every note already in the log still recomputes.
    #[n(4)]
    pub blob: Option<BlobRef>,
}

/// Bytes held outside the log, named by what they contain (§5.6).
///
/// # Why the content decides the name
///
/// The hash *is* the address, so verification is free — a replica that fetched
/// the wrong bytes cannot fail to notice — and duplicates are impossible: the
/// same recording captured twice is one blob with two references.
///
/// # Why the bytes are not in the event
///
/// A log is replicated in full to every device and is meant to be read in ten
/// years. Forty megabytes of audio inside an event would be sent to every
/// replica whether it wanted the recording or not, and could never be dropped
/// from a phone without dropping the note along with it. Keeping the reference
/// small and the bytes fetchable is what makes eviction possible at all
/// (§5.6) — a constrained device drops the bytes and keeps the note.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct BlobRef {
    /// `blake3` of the content.
    #[cbor(n(0), with = "minicbor::bytes")]
    pub hash: [u8; 32],
    /// What the bytes are, as an IANA media type.
    ///
    /// Recorded at capture because it cannot be recovered later with
    /// confidence, and a recording nothing can decide how to play is a
    /// recording nobody can hear.
    #[n(1)]
    pub media_type: String,
    /// How many bytes, so a device knows what it is agreeing to fetch before
    /// it starts, and can tell a truncated transfer from a complete one.
    #[n(2)]
    pub bytes: u64,
    /// How long it runs, in milliseconds, where that means anything.
    ///
    /// Written at capture by the device that recorded it. Not derived later
    /// from the bytes: a surface listing recordings should not have to fetch
    /// forty megabytes to find out one of them is nine seconds long.
    #[n(3)]
    pub duration_ms: Option<u32>,
}

/// Retrieval scaffolding gathered automatically at capture.
///
/// Every field is optional because availability is platform-dependent, and an
/// absent field must stay absent rather than becoming an empty string a query
/// could mistake for data.
#[derive(Clone, Default, PartialEq, Eq, Debug, Encode, Decode)]
pub struct Ambient {
    /// Foreground application. Unavailable on Wayland — no protocol exposes it.
    #[n(0)]
    pub app: Option<String>,
    /// Foreground window title. Same limitation.
    #[n(1)]
    pub window_title: Option<String>,
    /// Working directory. Available for terminal captures, where it is the
    /// capturing process's own — which is why the terminal is the better
    /// capture surface on platforms that hide window identity.
    #[n(2)]
    pub cwd: Option<String>,
    /// Git repository name.
    #[n(3)]
    pub repo: Option<String>,
    /// Git branch.
    #[n(4)]
    pub branch: Option<String>,
    /// Source URL, for share-style captures.
    #[n(5)]
    pub source_url: Option<String>,
    // Deliberately absent: clipboard contents. Copying the clipboard into an
    // append-only log is how a credential gets into it — the case redaction
    // exists for, and the one redaction cannot reach in written exports.
}

/// A human edit to a note's text.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct EditBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
    /// Replacement text.
    #[n(1)]
    pub text: String,
}

/// Add a tag.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct TagBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
    /// The tag.
    #[n(1)]
    pub tag: String,
}

/// Retract tag additions.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct UntagBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
    /// The specific `tag` events being retracted.
    ///
    /// Naming them individually is what makes tags an *observed-remove* set: a
    /// removal can only retract additions it actually saw, so a concurrent
    /// addition elsewhere survives rather than being silently undone.
    #[n(1)]
    pub kills: Vec<EventId>,
}

/// Suppress a note from review and every pushed surface.
///
/// Exists so that destruction is never the only way to stop seeing something.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct MuteBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
    /// What is being muted.
    #[n(1)]
    pub scope: MuteScope,
    /// Unix milliseconds after which the mute lapses. `None` is indefinite.
    #[n(2)]
    pub until: Option<u64>,
}

/// What a mute applies to.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub enum MuteScope {
    /// A single note.
    #[n(0)]
    Note(#[n(0)] EventId),
    /// Every note carrying a tag.
    #[n(1)]
    Tag(#[n(0)] String),
    /// Every note captured in a date range, as floating dates.
    #[n(2)]
    DateRange(#[n(0)] String, #[n(1)] String),
    /// Every note naming a person, project or tag.
    ///
    /// Required by §2.6's gradient and deferred until there were entities to
    /// scope to. Suppressing has to stay cheaper than destroying, and an entity
    /// somebody does not want to be reminded of is the case where the only
    /// other thing on offer would have been redaction — which is irreversible,
    /// and would take the notes rather than the reminders.
    ///
    /// By `ent_id`, so a rename does not silently un-mute what somebody muted.
    #[n(3)]
    Entity(#[n(0)] EntId),
}

/// Destroy content on every replica. The only irreversible operation.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct RedactBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
    /// What to destroy.
    #[n(1)]
    pub scope: RedactScope,
    /// Targets. Capped, because an uncapped redaction lets one loop destroy the
    /// whole archive everywhere within a single push round.
    #[n(2)]
    pub targets: Vec<EventId>,
}

/// Maximum targets a single redaction may name.
pub const REDACT_TARGET_CAP: usize = 64;

/// Whether a redaction takes whole notes or individual events.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Encode, Decode)]
pub enum RedactScope {
    /// Every event targeting each note — the default, because redacting only
    /// the current version leaves the sensitive content intact in the version
    /// that preceded it, which is the common real case.
    #[n(0)]
    WholeNote,
    /// Exactly the named events.
    #[n(1)]
    SingleEvent,
}

/// Abort a queued redaction inside its window.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct RedactCancelBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
    /// The redaction being cancelled.
    #[n(1)]
    pub redact: EventId,
}

/// Pin a note to the top, or unpin it.
///
/// Carries the value rather than meaning "pinned" by its existence, so
/// unpinning is possible. An event that only ever sets a flag needs a second
/// event kind to clear it, and two kinds for one boolean is a conflict rule
/// waiting to be got wrong.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct PinBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
    /// Whether the note is pinned after this event.
    #[n(1)]
    pub pinned: bool,
}

/// Give a note a chosen name, or take it away.
///
/// Aliases share one namespace with slugs, so a name denotes exactly one note
/// whichever kind of name it is. Two namespaces would let an alias shadow
/// another note's canonical name, after which a written link means different
/// things depending on which namespace the reader consults first.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct AliasBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
    /// The chosen name. `None` removes whatever alias the note had.
    #[n(1)]
    pub alias: Option<String>,
}

/// Give an entity a name somebody chose.
///
/// Only the display changes. The `ent_id` came from the first spelling seen and
/// is never recomputed (§4.3), which is what keeps a rename from forking the
/// page and leaving half a history behind under the old word.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct EntityRenameBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
    /// Which entity. Never a spelling — see [`EntId`].
    #[n(1)]
    pub ent: EntId,
    /// What to show. Empty is refused by the fold rather than displayed, since
    /// an entity with no name is one nothing can point at.
    #[n(2)]
    pub display: String,
}

/// Say that two entities were one all along.
///
/// A redirection, not a rewrite: nothing is moved, nothing is deleted, and no
/// mention row changes. Resolution follows the pointer, which is what makes a
/// merge survivable in a log that cannot take anything back — and is why this
/// carries two ids rather than a list of notes to reassign.
///
/// Never inferred. Two spellings are indistinguishable from two people by
/// phrasing alone, the same argument §3.6 makes for `carries_forward`.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct EntityMergeBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
    /// The entity that stops being its own.
    #[n(1)]
    pub from: EntId,
    /// The entity it becomes.
    #[n(2)]
    pub into: EntId,
}

/// Bind the note that *is* the project, rather than one that mentions it.
///
/// `None` unbinds, and is a decision rather than an absence — the same reason
/// [`ParentBody`] carries an `Option`. "No longer backed by anything" has to be
/// able to beat an earlier bind, and a body of this kind holding no note can
/// mean nothing else.
///
/// Binding does not retype the note. §4.3 puts this on the entity precisely so
/// that what a note can be stays `note | task | event | series`; a backing note
/// may be a task, and goes on being one everywhere tasks appear.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct EntityBindBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
    /// Which entity.
    #[n(1)]
    pub ent: EntId,
    /// The backing note, or `None` to unbind.
    #[n(2)]
    pub note: Option<EventId>,
}

/// Dismiss a divergence that has been looked at.
///
/// Naming the specific versions rather than saying "clear them all" — a
/// divergence that turned up after the user looked must not be swept away by a
/// dismissal authored before it existed.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct ResolveBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
    /// The competing versions being dismissed.
    #[n(1)]
    pub dismiss: Vec<EventId>,
}

/// Clear a note's credential flag.
///
/// Carries nothing but its salt: the note is named in the header, and there is
/// no value to set, because the flag only ever moves one way.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct UnflagBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
}

/// Record a preference the user actually chose.
///
/// Only ever written when someone makes a choice. Defaults stay compiled in and
/// unrecorded, because materialising them as events would freeze today's
/// judgement forever — every default could then never be improved without
/// contradicting a stored decision — and would destroy the one distinction that
/// matters here: whether a value is something you picked or something the
/// system picked for you.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct SettingBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
    /// The preference being set.
    #[n(1)]
    pub key: String,
    /// Its value. `None` restores the compiled-in default.
    #[n(2)]
    pub value: Option<String>,
}

/// Declare a new origin under an existing roster key, after a restore.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct OriginClaimBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
    /// The roster public key this origin belongs to.
    #[cbor(n(1), with = "minicbor::bytes")]
    pub pubkey: [u8; 32],
    /// The incarnation that produced this origin.
    #[cbor(n(2), with = "minicbor::bytes")]
    pub incarnation: [u8; 16],
    /// The X25519 public key this device answers a Noise handshake with (§5.2).
    ///
    /// A field added at the end, which is the one change a frozen body permits.
    /// Absent on every claim written before sync existed, and that absence is
    /// meaningful rather than a defect: such a device is in the roster and
    /// cannot complete a handshake, which is the true statement about it until
    /// it publishes one.
    ///
    /// Separate from the signing key on purpose. One key doing both signatures
    /// and Diffie-Hellman is the thing cryptographic hygiene warns about, and
    /// deriving one from the other would have saved a field and spent that.
    #[cbor(n(3), with = "minicbor::bytes")]
    pub noise: Option<[u8; 32]>,
}

/// Helper: build a body prefix from a supplied nonce.
///
/// `capture-core` cannot generate one — entropy needs a syscall — so this exists
/// to make the boundary obvious at every call site.
pub const fn nonce_bytes(n: Nonce) -> [u8; 16] {
    n.0
}

/// When something happens, in the one of two shapes it can be (§3.2).
///
/// An enum rather than a struct of optional fields, because "a timed event that
/// also carries all-day dates" is not a state that should be representable. The
/// two kinds of time are not interchangeable and the encoding says so.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub enum When {
    /// A moment. UTC milliseconds plus the zone it was written in — both are
    /// required, because UTC alone cannot expand a recurrence correctly.
    #[n(0)]
    Timed {
        /// Milliseconds since the Unix epoch, UTC.
        #[n(0)]
        start_ms: i64,
        /// When it ends, if that is known.
        #[n(1)]
        end_ms: Option<i64>,
        /// The IANA zone it was written in. Required — UTC alone cannot expand a
        /// recurrence correctly.
        #[n(2)]
        tz: String,
    },
    /// Floating dates, inclusive, with no zone and no conversion. A vacation
    /// from 1–14 August is 1–14 August everywhere.
    #[n(1)]
    AllDay {
        /// First day, inclusive.
        #[n(0)]
        start_date: String,
        /// Last day, inclusive.
        #[n(1)]
        end_date: String,
    },
}

/// A recurrence rule, as the log stores it.
///
/// The RFC 5545 subset of §3.4. Occurrences are never stored — a daily habit is
/// this one structure, not 365 events a year in a log that syncs to a phone.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct RuleSpec {
    /// 0 daily, 1 weekly, 2 monthly, 3 yearly. Numbers rather than a nested
    /// enum because this shape is permanent and a number is unambiguous.
    #[n(0)]
    pub freq: u8,
    /// Every *n*-th period.
    #[n(1)]
    pub interval: u32,
    /// `BYDAY` as a bitmask, Monday is bit 0. Zero means "the day the series
    /// starts on".
    #[n(2)]
    pub by_day: u8,
    /// `BYMONTHDAY`. Empty means "the day of the month the series starts on".
    #[n(3)]
    pub by_month_day: Vec<u8>,
    /// Stop after this many occurrences.
    #[n(4)]
    pub count: Option<u32>,
    /// Stop at this instant, inclusive.
    #[n(5)]
    pub until: Option<i64>,
}

/// A task: no time of its own, a state, and optionally a day it is due.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct TaskFields {
    /// Whether it is finished.
    #[n(0)]
    pub done: bool,
    /// A floating date, never an instant — "due Friday" is not 00:00 UTC on
    /// Friday (§3.6).
    #[n(1)]
    pub due: Option<String>,
    /// Position within whatever lists it sits in. Lists themselves are tags, so
    /// a task can be in several without being duplicated.
    #[n(2)]
    pub order: Option<i64>,
}

/// An event: something that occupies time and has no state.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct EventFields {
    /// When it happens.
    #[n(0)]
    pub when: When,
    /// Where it happens.
    #[n(1)]
    pub location: Option<String>,
    /// Who is expected.
    #[n(2)]
    pub attendees: Vec<String>,
}

/// A recurring series: a rule, and whether missing one leaves a debt.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct SeriesFields {
    /// The first occurrence.
    #[n(0)]
    pub when: When,
    /// How it repeats.
    #[n(1)]
    pub rule: RuleSpec,
    /// Whether uncompleted instances accumulate.
    ///
    /// False by default and **never set by the recognizer** — inferring "this
    /// one really matters" from phrasing is exactly the guess that must not be
    /// made silently. A missed walk is not a missed dose (§3.6).
    #[n(2)]
    pub carries_forward: bool,
    /// The series this one was split from, if it was (§3.5).
    ///
    /// Changing a recurring series from a given occurrence onward bounds the
    /// old rule and starts a new series rather than editing the rule in place.
    /// Editing in place would silently rewrite what the schedule *was* last
    /// March, and a log that revises its own history is an unreliable narrator
    /// — fatal for a system whose whole purpose is compensating for unreliable
    /// memory.
    ///
    /// Added after the first golden vectors were written, which is only safe
    /// because a trailing `None` is not encoded at all: every body that omits
    /// it produces the bytes it always did. See `a_trailing_absent_field_is_
    /// simply_absent`.
    #[n(3)]
    pub split_from: Option<EventId>,
}

/// What a capture was recognised as.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub enum Promoted {
    /// Explicitly not a task or an event. Demoting back to a plain note is a
    /// promotion like any other, so a wrong type is corrected rather than
    /// worked around.
    #[n(0)]
    Note,
    /// Something to do, with a state and no time of its own.
    #[n(1)]
    Task(#[n(0)] TaskFields),
    /// Something that occupies time and has no state.
    #[n(2)]
    Event(#[n(0)] EventFields),
    /// Something that repeats.
    #[n(3)]
    Series(#[n(0)] SeriesFields),
    /// A page: written to be read and revised, never read for a type.
    ///
    /// Only ever written pinned, by a person ("New page", "Make this a page").
    /// The pin is what keeps every later parse off it, so a page that mentions
    /// "every monday at 2pm" stays a page and puts nothing on the calendar.
    /// Added 2026-10-13; a build older than that holds the event as malformed.
    #[n(4)]
    Page,
}

impl Promoted {
    /// `note`, `task`, `event`, `series` or `page`: the one spelling every
    /// surface and the projection use for what a note is.
    pub fn kind(&self) -> &'static str {
        match self {
            Promoted::Note => "note",
            Promoted::Task(_) => "task",
            Promoted::Event(_) => "event",
            Promoted::Series(_) => "series",
            Promoted::Page => "page",
        }
    }
}

/// Assign a derived type to a capture, without touching what was captured.
///
/// Additive and reversible: the text is never rewritten, consumed or relocated,
/// which is what makes a wrong interpretation survivable (§3.1.3).
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct PromoteBody {
    #[cbor(n(0), with = "minicbor::bytes")]
    /// Hiding salt.
    pub nonce: [u8; 16],
    /// What it was recognised as.
    #[n(1)]
    pub as_: Promoted,
    /// Whether a person authored this.
    ///
    /// A pinned promotion beats a parser-authored one regardless of HLC — the
    /// one conflict rule in the system that is not plain last-write-wins. Under
    /// plain LWW a later reparse would silently revert a correction somebody
    /// already made, and losing trust that way is unrecoverable (§6).
    #[n(2)]
    pub pinned: bool,
    /// The grammar version this was parsed under; `None` when a person wrote it.
    ///
    /// Nothing in the system observes a build, so without this a note captured
    /// in March could never benefit from a grammar fix shipped in June. Startup
    /// reparses anything promoted under an older version.
    #[n(3)]
    pub grammar: Option<u32>,
    /// The confidence the recognizer had, where a recognizer produced it.
    ///
    /// Recorded rather than discarded because the bands were reasoned rather
    /// than measured, and this is what makes them checkable against real
    /// captures later.
    #[n(4)]
    pub confidence: Option<u32>,
    /// `Some(true)` where a person is giving up their correction.
    ///
    /// Found in use on 2026-10-13: a suggestion accepted by mistake pinned the
    /// wrong day, and a pin outranks every parser promotion for ever, so the
    /// text was corrected and nothing moved. A pin is a person's word, and only
    /// a person's word can withdraw it — so this is written by a person, carries
    /// a parser's reading of the text, and is not itself a pin: later parses
    /// displace it as they would any parser promotion. See `apply_promote`.
    ///
    /// Added after the golden vectors; a trailing `None` is not encoded, so every
    /// body that omits it is the bytes it always was.
    #[n(5)]
    pub released: Option<bool>,
}

/// What has to be true for an intent to fire.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub enum Cue {
    /// At an instant.
    #[n(0)]
    Time(#[n(0)] i64),
    /// On a schedule.
    #[n(1)]
    Recurring(#[n(0)] RuleSpec),
    /// Within range of somewhere. Only a phone can observe this (§2.4.1).
    ///
    /// Coordinates are micro-degrees rather than floats, and that is a decision
    /// about the encoding rather than about precision. This body is hashed into
    /// the event id: a float would make the id depend on two devices agreeing
    /// bit-for-bit about a decimal fraction, and it would put NaN — which is not
    /// equal to itself — inside a value that has to compare and converge.
    /// Integer micro-degrees are exact, about 11cm at the equator, and cannot
    /// be any of those things.
    #[n(2)]
    Location {
        /// Degrees north, times a million.
        #[n(0)]
        lat_e6: i32,
        /// Degrees east, times a million.
        #[n(1)]
        lon_e6: i32,
        /// How close counts, in metres.
        #[n(2)]
        radius_m: u32,
    },
    /// When an application takes focus. Fires on the machine running it —
    /// "check #incidents when you open Slack" is useless on a phone while you
    /// are sitting at the desktop.
    #[n(3)]
    AppFocus(#[n(0)] String),
    /// When a calendar entry starts.
    #[n(4)]
    CalendarStart(#[n(0)] Option<EventId>),
    /// When a device wakes.
    #[n(5)]
    DeviceWake,
    /// When a repository is on a branch. Fires where the repository is.
    #[n(6)]
    GitBranch(#[n(0)] String),
}

/// An if-then plan (§2.4).
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct IntentBody {
    /// Hiding salt.
    #[cbor(n(0), with = "minicbor::bytes")]
    pub nonce: [u8; 16],
    /// What has to be true.
    #[n(1)]
    pub cue: Cue,
    /// What to do when the cue fires.
    #[n(2)]
    pub then: String,
}

/// What became of a resurfaced note.
///
/// Not `recalled / missed / snoozed`. That is a graded memory test, on a
/// schedule the user did not set, written permanently to an immutable log and
/// fed back into scheduling — for a person whose stated difficulty is memory.
/// §2.7 withdrew it, and the encoding is where that becomes unavailable rather
/// than merely discouraged.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Encode, Decode)]
pub enum Outcome {
    /// It appeared.
    #[n(0)]
    Shown,
    /// Something was done with it.
    #[n(1)]
    Engaged,
    /// It was pushed away.
    #[n(2)]
    Deferred,
}

/// A scheduled resurfacing happened (§2.8).
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct ReviewBody {
    /// Hiding salt.
    #[cbor(n(0), with = "minicbor::bytes")]
    pub nonce: [u8; 16],
    /// What became of it.
    #[n(1)]
    pub outcome: Outcome,
}

/// This occurrence only: a new time.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct OccMoveBody {
    /// Hiding salt.
    #[cbor(n(0), with = "minicbor::bytes")]
    pub nonce: [u8; 16],
    /// The occurrence's name: the instant the rule would have produced, before
    /// any move (§3.5). Stable — moving an occurrence never renames it.
    #[n(1)]
    pub occ: i64,
    /// Where it moves to.
    #[n(2)]
    pub start_ms: i64,
    /// Where it now ends, if that is known.
    #[n(3)]
    pub end_ms: Option<i64>,
}

/// This occurrence only: different details.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct OccEditBody {
    /// Hiding salt.
    #[cbor(n(0), with = "minicbor::bytes")]
    pub nonce: [u8; 16],
    /// Which occurrence, by its original start (§3.5).
    #[n(1)]
    pub occ: i64,
    /// A different title, for this occurrence only.
    #[n(2)]
    pub title: Option<String>,
    /// A different location, for this occurrence only.
    #[n(3)]
    pub location: Option<String>,
    /// Notes on this occurrence only.
    #[n(4)]
    pub notes: Option<String>,
}

/// One occurrence, named. Used by cancel, done and undone, which need nothing
/// beyond which occurrence they mean.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct OccBody {
    /// Hiding salt.
    #[cbor(n(0), with = "minicbor::bytes")]
    pub nonce: [u8; 16],
    /// Which occurrence, by its original start (§3.5).
    #[n(1)]
    pub occ: i64,
}

/// One item of a list, named. Used by done and undone, which need nothing
/// beyond which item they mean.
///
/// Named by **text** rather than by position, which is the whole of the
/// design. Inserting an item moves every item after it, so a completion bound
/// to an index would silently move to a different item the first time somebody
/// edited a list — the same failure `OccBody` avoids by naming an occurrence
/// with the instant its rule would have produced rather than its index (§3.5).
///
/// The cost, stated because it is real: two items on one list reading the same
/// text cannot be told apart. The surfaces refuse to guess rather than picking
/// one.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct ItemBody {
    /// Hiding salt.
    #[cbor(n(0), with = "minicbor::bytes")]
    pub nonce: [u8; 16],
    /// The words written for the item.
    ///
    /// No longer the identity, and still here for two reasons. It is what lets
    /// a tick survive an item moving — the words appearing once elsewhere is a
    /// better answer than a position that has shifted under them — and it is
    /// what keeps `capture log` legible, since an event saying "item 2 done"
    /// and nothing else is unreadable by somebody trying to understand their
    /// own archive.
    #[n(1)]
    pub item: String,
    /// Which item, by where it was written.
    ///
    /// Added at a new index, which is the one permitted evolution of a frozen
    /// body: an older build reading this gets the text it already understood,
    /// and an event written before this arrives here as `None`.
    ///
    /// Position rather than text, because text is not an identity. Correcting a
    /// word on a shopping list used to discard what had already been bought —
    /// the `item_done` event stayed in the log and simply stopped matching
    /// anything, which §6 rates worse than a state that cannot be reversed,
    /// because nothing said it had happened.
    ///
    /// Not an identity either, and nothing here can be: items are re-derived
    /// from a mutable string on every fold, so there is nothing minted to point
    /// at. Position and text together are a *reconciliation*, and the rules are
    /// in the store, which is the only layer that can expand a capture's items.
    #[n(2)]
    pub ord: Option<u32>,
    /// How many items the capture had when this was written.
    ///
    /// What makes the position usable. Without it, falling back to a position
    /// cannot tell a *reworded* item from a *deleted* one: remove the middle of
    /// three and the position still exists, now holding the item that was
    /// below it, which then inherits a tick nobody gave it.
    ///
    /// With it the fallback is only taken when the count is unchanged — so a
    /// reword is matched and a deletion is not matched at all. The tick then
    /// applies to nothing, which is the honest outcome: the event is still in
    /// the log, and restoring the item restores it.
    ///
    /// Found by a test. The rule as first written handed a deleted item's tick
    /// to its neighbour.
    #[n(3)]
    pub of: Option<u32>,
}

/// Where a page sits in the wiki tree.
///
/// # The one hand-maintained structure here
///
/// §3.7's argument stands and is not withdrawn: collections are saved queries
/// because a filing ritual has an excellent chance of not happening, and a
/// hierarchy is filing. This exists because it was asked for with that argument
/// in front of it, and the mitigation is containment — nothing outside the wiki
/// sidebar reads a parent. A page filed wrongly is in the wrong branch of one
/// list; it is still in the timeline, still in search, still on every entity
/// page that names it, and still reachable by every link pointing at it. The
/// failure mode §3.7 fears is "I filed it and now it is gone", and that cannot
/// happen to something you cannot remove from anywhere by filing it.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct ParentBody {
    /// Hiding salt.
    #[cbor(n(0), with = "minicbor::bytes")]
    pub nonce: [u8; 16],
    /// The note this one sits under. `None` puts it back at the top, which is
    /// why it is an option rather than an absence — "no longer under anything"
    /// is a decision somebody made, and it has to be able to beat an earlier
    /// parent by ordinary last-write-wins.
    #[n(1)]
    pub parent: Option<EventId>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::BodyHash;
    use alloc::string::ToString;

    fn capture(nonce: u8, text: &str) -> CaptureBody {
        CaptureBody {
            nonce: [nonce; 16],
            text: text.to_string(),
            tz: Some("Europe/London".to_string()),
            ambient: Ambient::default(),
            blob: None,
        }
    }

    fn canonical<T: Encode<()>>(v: &T) -> Vec<u8> {
        let mut b = Vec::new();
        minicbor::encode(v, &mut b).unwrap();
        b
    }

    #[test]
    fn identical_text_gives_different_commitments() {
        // The property the nonce exists for: without it, bhash would be an
        // unsalted commitment to content redaction is meant to destroy.
        let a = BodyHash::of(&canonical(&capture(1, "hunter2")));
        let b = BodyHash::of(&canonical(&capture(2, "hunter2")));
        assert_ne!(a, b);
    }

    #[test]
    fn commitment_is_stable_for_identical_input() {
        let a = BodyHash::of(&canonical(&capture(1, "milk")));
        let b = BodyHash::of(&canonical(&capture(1, "milk")));
        assert_eq!(a, b);
    }

    #[test]
    fn bodies_round_trip() {
        let c = capture(5, "budget 40k, revisit Q2");
        let bytes = canonical(&c);
        let back: CaptureBody = minicbor::decode(&bytes).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn absent_ambient_stays_absent() {
        // An unavailable field must not become an empty string a query could
        // mistake for real data.
        let c = capture(1, "x");
        assert!(c.ambient.app.is_none());
        let back: CaptureBody = minicbor::decode(&canonical(&c)).unwrap();
        assert!(back.ambient.app.is_none());
    }

    #[test]
    fn text_survives_encoding_byte_for_byte() {
        let weird = "line1\nline2\t✓ 日本語 #tag >2026-08-24 `date` $VAR";
        let c = capture(1, weird);
        let back: CaptureBody = minicbor::decode(&canonical(&c)).unwrap();
        assert_eq!(back.text, weird);
    }
}

/// The body of a `roster_key` event: the shared secret, minted or rotated.
///
/// Deliberately not a field on [`OriginClaimBody`], though the roster root is
/// where the first one appears. That body is shared by `device_add` and
/// `origin_claim`, and a secret on an origin claim would be meaningless — a
/// field that must always be absent on one of its two kinds is a rule to
/// enforce and test, where a separate body cannot express the mistake.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct RosterKeyBody {
    /// Hiding salt.
    #[cbor(n(0), with = "minicbor::bytes")]
    pub nonce: [u8; 16],
    /// The secret itself. Every member holds it; membership is what holding it
    /// means.
    #[cbor(n(1), with = "minicbor::bytes")]
    pub secret: [u8; 32],
}

/// One origin's last legitimate event, as the retiring device saw it.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct RetiredHead {
    /// The origin.
    #[n(0)]
    pub dev: Dev,
    /// Its last sequence number that is to be honoured.
    #[n(1)]
    pub seq: u64,
    /// And that event's id, so the head is checkable rather than merely
    /// asserted — a sequence number alone could be claimed by a forged event
    /// occupying the position.
    #[n(2)]
    pub last: EventId,
}

/// The body of a `device_retire`.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct RetireBody {
    /// Hiding salt.
    #[cbor(n(0), with = "minicbor::bytes")]
    pub nonce: [u8; 16],
    /// The key being retired. Not an origin: retiring an origin would let a
    /// stolen device mint a fresh incarnation and return.
    #[cbor(n(1), with = "minicbor::bytes")]
    pub pubkey: [u8; 32],
    /// A head per origin the issuer knows that key to have authored under.
    #[n(2)]
    pub heads: Vec<RetiredHead>,
    /// A prior retirement this one replaces.
    ///
    /// Two rules in §5.3 are in tension without this. Between *concurrent*
    /// retirements the lowest head wins, because that is the safe direction —
    /// it excludes the events whose provenance is in doubt. But a retirement
    /// may also be *reissued* naming a later head, when the retired device
    /// turns out to have propagated events past the first one, and under
    /// lowest-wins a reissue could never widen anything.
    ///
    /// Naming the retirement being replaced separates the two cases explicitly
    /// rather than hoping arrival order distinguishes them — which it cannot,
    /// since there is no causal ordering to read off a timestamp. A reissue is
    /// therefore a deliberate decision about what to keep, which is what the
    /// design says it is.
    #[n(3)]
    pub supersedes: Option<EventId>,
}

/// The body of a `device_readmit`: an admission signed by two keys.
///
/// The second signature cannot cover the event id, because the id covers the
/// body hash and the body is where the second signature lives. It covers the
/// subject instead — see [`readmit_digest`] — which is the thing that actually
/// needs two people to agree on.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct ReadmitBody {
    /// Hiding salt.
    #[cbor(n(0), with = "minicbor::bytes")]
    pub nonce: [u8; 16],
    /// The key being re-admitted.
    #[cbor(n(1), with = "minicbor::bytes")]
    pub pubkey: [u8; 32],
    /// The incarnation whose origin is being re-admitted with it.
    #[cbor(n(2), with = "minicbor::bytes")]
    pub incarnation: [u8; 16],
    /// The second signer's key. May itself be retired — that is the point.
    #[cbor(n(3), with = "minicbor::bytes")]
    pub cosigner: [u8; 32],
    /// The second signature, over [`readmit_digest`].
    #[cbor(n(4), with = "minicbor::bytes")]
    pub cosign: [u8; 64],
}

/// What a re-admission's second signer signs.
///
/// Domain-separated, so a signature harvested from anywhere else in the system
/// cannot be replayed as a co-signature — which would reduce "two devices
/// agreeing" back to one.
pub fn readmit_digest(pubkey: &[u8; 32], incarnation: &[u8; 16], nonce: &[u8; 16]) -> [u8; 32] {
    let mut h = blake3::Hasher::new();
    h.update(b"bsp-readmit");
    h.update(pubkey);
    h.update(incarnation);
    h.update(nonce);
    *h.finalize().as_bytes()
}
