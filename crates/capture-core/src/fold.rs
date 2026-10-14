//! The fold: events in, state out.
//!
//! # The one rule
//!
//! **This function takes the event set and nothing else.** No clock, no
//! configuration, no roster as it currently stands, no filesystem. Everything it
//! needs arrives in the argument.
//!
//! That is what makes several devices agree. Each holds the same events and
//! computes the same answer, without coordinating. Let anything else in and two
//! devices can disagree while each remains internally consistent — which is a
//! defect no test on either machine can see.
//!
//! # Order independence
//!
//! Events arrive in whatever order the network delivers them. So the fold sorts
//! by timestamp first and reads that sorted sequence, rather than reacting to
//! arrival. The observable consequence, and the property most worth testing:
//! shuffling the input cannot change the output.
//!
//! # What comes out
//!
//! A plain data structure, not a database. Two reasons. Comparing two folds for
//! equality is then meaningful — two database files are never byte-identical,
//! because row identifiers and page layout depend on insertion order, so
//! "identical output" could not be asserted against storage. And keeping the
//! rules separate from the writing means they can be exercised with no runtime
//! at all.
//!
//! Every collection here is ordered rather than hashed, so the encoding depends
//! on content instead of on iteration order.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::vec::Vec;

use minicbor::{Decode, Encode};

use crate::{
    body::{
        AliasBody, CaptureBody, EditBody, EntityBindBody, EntityMergeBody, EntityRenameBody,
        ItemBody, MuteBody, MuteScope, OccBody, OccEditBody, OccMoveBody, Outcome, ParentBody,
        PinBody, PromoteBody, RedactBody, RedactCancelBody, ResolveBody, ReviewBody, SettingBody,
        TagBody, UntagBody,
    },
    envelope::Envelope,
    event::Kind,
    hlc::Hlc,
    id::{Dev, EntId, EventId},
    link::{self, Link},
    slug,
};

/// One version of a note, and the version it superseded.
///
/// Retained for every version, including ones excluded from the fold. Dropping
/// an excluded version's edge would leave its successors pointing at nothing —
/// and a walk that stops early reports edits made in sequence as though they
/// had been made independently, which is the exact error the walk exists to
/// avoid.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct Version {
    /// This version's event.
    #[n(0)]
    pub id: EventId,
    /// The version it supersedes. Absent on the note's first event.
    #[n(1)]
    pub prev: Option<EventId>,
    /// When it was authored.
    #[n(2)]
    pub hlc: Hlc,
    /// What kind of change it was.
    #[n(3)]
    pub kind: Kind,
    /// Whether it was excluded from the fold.
    #[n(4)]
    pub excluded: bool,
}

/// Two versions that were authored independently.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Encode, Decode)]
pub struct Divergence {
    /// The version the fold chose.
    #[n(0)]
    pub current: EventId,
    /// The version it did not choose. Kept, never discarded.
    #[n(1)]
    pub other: EventId,
}

/// A preference someone chose, and who chose it.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct Setting {
    /// The chosen value. `None` means the user explicitly reverted to the
    /// default, which is a different act from never having chosen.
    #[n(0)]
    pub value: Option<String>,
    /// The origin that made the choice.
    #[n(1)]
    pub set_by: Dev,
    /// When, so a later choice wins.
    #[n(2)]
    pub at: Hlc,
}

/// A redaction as the fold collected it.
#[derive(Clone, Copy)]
struct Ordered {
    event: EventId,
    at: Hlc,
    window_ms: u64,
    cancelled: bool,
}

/// A redaction that has been ordered but not yet carried out.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Encode, Decode)]
pub struct Pending {
    /// The redaction event.
    #[n(0)]
    pub redact: EventId,
    /// When it becomes due, in milliseconds.
    ///
    /// Computed from the **redaction's own timestamp**, never from when it
    /// arrived here. Taking it from local receipt would hand every replica its
    /// own fresh window, so a redaction issued a month ago would wait another
    /// full window on a device that has been switched off — and the longer a
    /// replica was away, the longer the content it was supposed to destroy
    /// would survive.
    #[n(1)]
    pub execute_after: u64,
}

/// A note's current state.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct Note {
    /// The note's identity — its first event.
    #[n(0)]
    pub tgt: EventId,
    /// Derived name. Immutable, so links written against it never move.
    #[n(1)]
    pub slug: String,
    /// A chosen name, sharing one namespace with slugs.
    #[n(14)]
    pub alias: Option<String>,
    /// Whether the capture looked like it contained a credential.
    ///
    /// Decided once, from the capture text, by a predicate tuned to over-flag:
    /// a false positive costs a less readable name and a note left out of
    /// backups until someone says otherwise, while a false negative writes
    /// credential-shaped words into every note that links here, with no cleanup
    /// path. Cleared by an `unflag` event.
    #[n(17)]
    pub credential: bool,

    /// When that name was assigned.
    ///
    /// Needed so an incremental fold can seed the name correctly: a link
    /// written before the alias existed must not resolve through it, and
    /// without the moment of assignment there is nothing to compare against.
    #[n(16)]
    pub alias_at: Option<Hlc>,
    /// Current text. Absent once redacted.
    #[n(2)]
    pub text: Option<String>,
    /// The version currently shown.
    #[n(3)]
    pub head: EventId,
    /// When the note was captured.
    #[n(4)]
    pub created: Hlc,
    /// When it last changed.
    #[n(5)]
    pub updated: Hlc,
    /// Capture timezone, needed to group by the day it was captured on.
    #[n(6)]
    pub tz: Option<String>,
    /// What was going on when it was captured.
    ///
    /// Frequently the recognition cue when the text is not, which is the only
    /// reason it is collected — so it is carried through to the projection
    /// rather than left in the event body where a view cannot reach it.
    #[n(19)]
    pub ambient: crate::body::Ambient,
    /// Current tags.
    #[n(7)]
    pub tags: BTreeSet<String>,
    /// The `tag` events still standing behind each tag.
    ///
    /// Kept because an `untag` retracts *specific additions*, not a word. A
    /// device that removed a tag it could see must not also remove one added
    /// elsewhere that it never saw, and the only way to express that is to name
    /// the additions being retracted.
    #[n(15)]
    pub tag_adds: Vec<(String, EventId)>,
    /// Whether it is pinned.
    #[n(8)]
    pub pinned: bool,
    /// Suppressed from surfacing until this time. Zero means indefinitely.
    #[n(9)]
    pub muted_until: Option<u64>,
    /// When the mute that decided [`Note::muted_until`] was written.
    ///
    /// Here so that a mute resolved outside this crate can be ordered against
    /// the ones resolved inside it. An entity-scoped mute needs the note's
    /// text to know whether it applies, which this crate cannot read, so it is
    /// applied where mentions are computed — and without a timestamp to compare
    /// against it would win over every note-scoped mute simply by being applied
    /// later.
    #[n(26)]
    pub muted_at: Option<Hlc>,
    /// When its content was actually destroyed, if it has been.
    ///
    /// Set from the *absence of the body*, not from the existence of a
    /// redaction event. The two are different facts and the gap between them is
    /// the cancellation window: a redaction that has been issued but not yet
    /// carried out leaves the note perfectly readable, which is what makes
    /// cancelling it meaningful.
    #[n(10)]
    pub redacted_at: Option<Hlc>,
    /// A redaction that has been issued and not yet carried out.
    ///
    /// Carries the event that ordered it and the moment it becomes due, so a
    /// view can show a countdown and a cancellation has something to name.
    #[n(18)]
    pub redaction_pending: Option<Pending>,
    /// What this note was recognised as, if anything (§3.1).
    ///
    /// Absent means an ordinary note, which is the majority and is not a
    /// failure — a capture nothing could parse is still fully captured,
    /// searchable and surfaced.
    #[n(20)]
    pub promotion: Option<Promotion>,
    /// The latest promotion a person wrote: a correction, or the giving up of
    /// one. With [`Note::by_parser`], what decides [`Note::promotion`].
    #[n(28)]
    pub by_person: Option<Promotion>,
    /// The latest promotion a parser wrote.
    #[n(29)]
    pub by_parser: Option<Promotion>,
    /// What review has done with this note.
    ///
    /// Counts of engagement, and nothing about memory. §2.7 withdrew the graded
    /// outcome deliberately: a recurring, permanently-recorded record of recall
    /// failures, on a schedule the user did not set, is the single most harmful
    /// thing this design could contain for the person it is for.
    #[n(22)]
    pub review: ReviewState,
    /// The note this one sits under in the wiki, if somebody put it there.
    ///
    /// Read by one sidebar and nothing else, which is what makes it a bounded
    /// departure from §3.7 rather than a filing system: a page under the wrong
    /// parent is still in the timeline, still in search, still on every entity
    /// page that names it and still reachable by every link pointing at it.
    #[n(23)]
    pub parent: Option<EventId>,
    /// When the parent was last set, for last-write-wins and for breaking
    /// cycles by dropping the newest edge in one.
    #[n(24)]
    pub parent_at: Option<Hlc>,
    /// Per-occurrence exceptions to this note's series, keyed by original start.
    ///
    /// Only ever populated for a promoted series. An exception naming an
    /// occurrence the rule does not produce is kept rather than discarded: the
    /// rule can be edited later, and a completion that vanished because the
    /// schedule moved would be a completion silently lost.
    #[n(21)]
    pub occurrences: BTreeMap<i64, OccState>,
    /// Every tick written against this capture's items, in event order.
    ///
    /// Raw, and deliberately not reconciled here. Deciding *which* item a tick
    /// belongs to means expanding the capture's text, `list_of` lives in
    /// `capture-parse`, and `capture-parse` already depends on this crate — so
    /// this carries what was recorded and the projection decides what it meant,
    /// in the one place that holds both halves.
    ///
    /// A list rather than a map, because the key is the question. Keyed by text
    /// it used to be, and correcting a word then discarded the tick; keyed by
    /// position alone, inserting an item would move every tick below it to the
    /// wrong row. Both are kept and weighed where the items are known.
    ///
    /// Order is event order, so a later tick beats an earlier one on the same
    /// item without anything here having to know which items those are.
    #[n(25)]
    pub item_ticks: Vec<ItemTick>,
    /// Independent versions the fold could not order by causality.
    #[n(11)]
    pub divergences: Vec<Divergence>,
    /// Every version, including excluded ones.
    #[n(12)]
    pub versions: Vec<Version>,
    /// Links written in the version currently shown, each carrying the name as
    /// written and what it meant when it was written.
    #[n(13)]
    pub links: Vec<Link>,
    /// A redaction whose cancellation arrived after it had been carried out.
    ///
    /// §1.1 asks for a cancel arriving after its target has been destroyed to
    /// be "recorded and reported rather than silently discarded". It was
    /// recorded — the event is in the log like any other — and reported
    /// nowhere, because the fold computed both halves and kept neither: the
    /// content is gone, the governing order was withdrawn, and the destroyed
    /// branch simply set `redacted_at`.
    ///
    /// The situation is ordinary. Two devices, one redaction: you cancel on the
    /// phone and the desktop has already destroyed. The phone says nothing was
    /// destroyed, the desktop silently keeps it gone, and nothing reconciles
    /// the two. Somebody who cancels believes the content survived, and §6
    /// rates being wrong about that as unrecoverable.
    #[n(27)]
    pub cancelled_too_late: Option<EventId>,
}

impl Note {
    /// Whether the note has been redacted.
    pub fn is_redacted(&self) -> bool {
        self.redacted_at.is_some()
    }

    /// The text a reader may see, which is not always the text the note has.
    ///
    /// `None` once the content is destroyed, and `None` from the moment the
    /// destruction is *ordered*. §1.1 withholds then rather than at the end of
    /// the window — the window is cancellable precisely because the content is
    /// already unreadable, so it is a chance to change your mind and not a
    /// delay before anything happens.
    ///
    /// The fold keeps `text` through the window on purpose, so a cancellation
    /// has something to restore; this answers the different question of whether
    /// anybody may be shown it.
    ///
    /// Exists because that question was being decided separately by every table
    /// derived from the text, and three of them got it wrong. `mentions` and
    /// `list_items` tested `text.is_some()`, which is true for the whole
    /// window, so a note somebody had asked to destroy went on naming people
    /// and listing its contents; `links` tested nothing at all and kept the
    /// words inside `[[..]]`. Only the search index had it right, by writing
    /// this condition inline — and the two read-time constants that withhold
    /// `notes.text` each carry a comment about a hand-written copy of a rule
    /// that was "present in two call sites and absent from the rest".
    pub fn readable_text(&self) -> Option<&str> {
        self.text
            .as_deref()
            .filter(|_| self.redaction_pending.is_none())
    }
}

/// A derived type, and where it came from.
///
/// The captured text is untouched by any of this. A promotion annotates a note;
/// it never rewrites, consumes or relocates what was typed, which is what makes
/// a wrong interpretation survivable rather than lossy (§3.1.3).
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct Promotion {
    /// What it was recognised as.
    #[n(0)]
    pub as_: crate::body::Promoted,
    /// Whether a person authored it.
    #[n(1)]
    pub pinned: bool,
    /// The grammar version behind it, where a recognizer produced it.
    #[n(2)]
    pub grammar: Option<u32>,
    /// The confidence the recognizer had, where a recognizer produced it.
    #[n(3)]
    pub confidence: Option<u32>,
    /// When it was assigned. The tie-breaker among promotions of equal
    /// authority, and deliberately not the tie-breaker across authorities.
    #[n(4)]
    pub at: Hlc,
    /// The event that assigned it.
    #[n(5)]
    pub event: EventId,
}

/// An if-then plan.
///
/// Cheap to make and cheap to abandon, which is most of the honest case for
/// having them at all: §2.4 is explicit that the effect is small and the
/// mechanism is easy to walk away from if it does no good.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct Intent {
    /// What has to be true.
    #[n(0)]
    pub cue: crate::body::Cue,
    /// What to do about it.
    #[n(1)]
    pub then: String,
    /// When it was made.
    #[n(2)]
    pub at: Hlc,
    /// Whether it has been abandoned.
    ///
    /// Retained rather than removed. A plan somebody gave up on is a fact about
    /// what they tried, and the log does not forget things merely because they
    /// stopped applying.
    #[n(3)]
    pub retracted: bool,
}

/// What review has done with a note.
///
/// Deliberately not a score. These are inputs to scheduling — §2.8 tunes
/// intervals by engagement, and an item deferred twice stops being offered —
/// and they are never a number shown to anybody. The distinction is the whole
/// of §2.7: engagement is a fact about what you did, and recall is a judgement
/// about your memory, which this system does not record.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Encode, Decode)]
pub struct ReviewState {
    /// When it was last offered.
    #[n(0)]
    pub last_shown: Option<Hlc>,
    /// How many times something was done with it.
    #[n(1)]
    pub engaged: u32,
    /// How many times it was pushed away.
    ///
    /// Twice is the limit, and then it stops being offered rather than being
    /// pressed. Somebody who has declined an item twice has told you.
    #[n(2)]
    pub deferred: u32,
}

/// How many refusals end the offering.
///
/// Twice. Somebody who has declined an item twice has told you, and §2.7 has no
/// graded outcome that a third ask could be justified by.
///
/// Public because the projection's review query needs the same number, and it
/// used to have its own `< 2` written into the SQL beside its own copy of the
/// resurfacing ladder. One number, one place: the review schedule has already
/// been two copies once, and that is the shape `tools/unwired.py` exists for.
pub const DECLINED_AFTER: u32 = 2;

impl ReviewState {
    /// Whether review should stop offering this.
    pub fn declined(&self) -> bool {
        self.deferred >= DECLINED_AFTER
    }
}

/// What has happened to one occurrence of a series (§3.5).
///
/// Named by `(series, original_start)` — the instant the rule would have
/// produced, before any move. Moving an occurrence changes where it is shown
/// and never what it is called, which is what lets an exception written by
/// another device still find it.
#[derive(Clone, PartialEq, Eq, Debug, Default, Encode, Decode)]
pub struct OccState {
    /// Moved to a different time, if it has been.
    #[n(0)]
    pub moved_to: Option<(i64, Option<i64>)>,
    /// Skipped, at some point. Whether that still holds depends on what came
    /// after it — see [`OccState::visible`].
    #[n(1)]
    pub cancelled: bool,
    /// Completed, for a recurring obligation (§3.6).
    #[n(2)]
    pub done: bool,
    /// A different title for this occurrence only.
    #[n(3)]
    pub title: Option<String>,
    /// A different location for this occurrence only.
    #[n(4)]
    pub location: Option<String>,
    /// Notes on this occurrence only.
    #[n(5)]
    pub notes: Option<String>,
    /// Whether the latest of its moves and cancellations was a cancellation.
    ///
    /// The fold reads events in a total order, so "latest" is the same answer
    /// on every device whatever order they arrived in.
    #[n(6)]
    pub skipped_last: bool,
}

impl OccState {
    /// Whether this occurrence should be drawn.
    ///
    /// A cancelled occurrence is hidden, not gone: the cancellation is an
    /// ordinary event and nothing has been destroyed. A move made after a
    /// cancellation brings it back — somebody wanted it to happen after all,
    /// and Restore is exactly that. A cancellation made after a move hides it
    /// again: otherwise the gesture for dismissing a resurrected occurrence
    /// would be the one gesture that cannot.
    pub fn visible(&self) -> bool {
        !self.cancelled || (self.moved_to.is_some() && !self.skipped_last)
    }
}

/// Why an event was held back from the fold.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Encode, Decode)]
pub enum Held {
    /// This build does not recognise the event kind.
    #[n(0)]
    UnknownKind,
    /// The payload could not be decoded.
    #[n(1)]
    Malformed,
    /// The note it refers to is not present.
    #[n(2)]
    NoTarget,
}

/// The computed state.
#[derive(Clone, PartialEq, Eq, Debug, Default, Encode, Decode)]
pub struct Projection {
    /// Notes, keyed by identity.
    #[n(0)]
    pub notes: BTreeMap<EventId, Note>,
    /// Events held back, and why.
    #[n(1)]
    pub held: BTreeMap<EventId, Held>,
    /// Preferences the user has actually chosen. A key that is absent has
    /// never been chosen and takes its compiled-in default — which is exactly
    /// the distinction this map exists to preserve.
    #[n(2)]
    pub settings: BTreeMap<String, Setting>,
    /// If-then plans, keyed by the event that created each one (§2.4).
    ///
    /// Not attached to a note. An intent is a plan about a situation rather
    /// than a fact about a capture, and hanging it off a note would make
    /// abandoning the note abandon the plan.
    #[n(3)]
    pub intents: BTreeMap<EventId, Intent>,
    /// Entity-scoped mutes, carried out whole because this crate cannot decide
    /// which notes they cover.
    ///
    /// Which notes name an entity is a question about their text, and answering
    /// it needs `capture_parse::mentions` — which depends on this crate. So the
    /// rule travels with the moment it was written, and is applied where
    /// mentions are already computed.
    ///
    /// A `Vec` rather than a map, and in event order: two mutes of one entity
    /// resolve newest-wins the same way every other scope does, which needs the
    /// order kept rather than the key deduplicated.
    #[n(4)]
    pub entity_mutes: Vec<(EntId, Option<u64>, Hlc)>,
    /// Names somebody chose for an entity, by identity rather than by spelling.
    ///
    /// Absent means nobody has renamed it, and the spelling written first is
    /// shown instead — which is decided where mentions are computed, since that
    /// is the only place that knows what the text says.
    #[n(5)]
    pub entity_display: BTreeMap<EntId, (String, Hlc)>,
    /// Which entity another turned out to be.
    ///
    /// A chain, not a single hop: A into B and B into C leaves both edges here,
    /// and resolution walks to the end. Cycles are broken after the main pass
    /// for the same reason parent cycles are.
    #[n(6)]
    pub entity_merged: BTreeMap<EntId, (EntId, Hlc)>,
    /// The note that *is* this entity, where one has been bound.
    ///
    /// `Option` inside the value because unbinding is a decision that has to be
    /// able to beat an earlier bind, and the `Hlc` is what decides between them.
    #[n(7)]
    pub entity_backing: BTreeMap<EntId, (Option<EventId>, Hlc)>,
    /// Tag mutes, carried whole for the notes that tagged themselves inline.
    ///
    /// The fold applies a tag mute to notes in the tag OR-Set, which only
    /// `capture tag` writes. A `#noise` typed into a capture is a mention in
    /// the text, and finding it needs `capture_parse` — which depends on this
    /// crate. So the rule travels on, as entity mutes do, and the store applies
    /// it where the mentions are known. Without this the ordinary way to tag
    /// something was the one way muting the tag did not reach.
    #[n(8)]
    pub tag_mutes: Vec<(String, Option<u64>, Hlc)>,
}

impl Projection {
    /// Which entity a name's id ends up being, after every merge.
    ///
    /// Walks the chain to its end. The bound is the number of edges: cycles are
    /// broken during the fold, so one cannot be reached here, and the bound is
    /// what makes that a guarantee rather than a hope — a caller holding a
    /// projection assembled some other way still terminates.
    pub fn entity_of(&self, ent: EntId) -> EntId {
        let mut at = ent;
        for _ in 0..=self.entity_merged.len() {
            match self.entity_merged.get(&at) {
                Some((next, _)) => at = *next,
                None => return at,
            }
        }
        at
    }

    /// What to show for an entity, where somebody has chosen a name.
    ///
    /// `None` means nobody has, and the spelling as written is what to use —
    /// which this crate cannot know, because it is in the note's text.
    pub fn entity_name(&self, ent: EntId) -> Option<&str> {
        self.entity_display
            .get(&self.entity_of(ent))
            .map(|(d, _)| d.as_str())
    }

    /// The note bound as an entity's backing note, if one is.
    pub fn entity_backing_of(&self, ent: EntId) -> Option<EventId> {
        self.entity_backing
            .get(&self.entity_of(ent))
            .and_then(|(n, _)| *n)
    }

    /// Canonical bytes. Two folds of the same events encode identically.
    pub fn canonical(&self) -> Vec<u8> {
        let mut b = Vec::new();
        minicbor::encode(self, &mut b).expect("encoding into Vec is infallible");
        b
    }
}

/// One tick, as it was written.
///
/// Both halves of how an item was identified, kept so the projection can weigh
/// them: the position it was written at, and the words it had at the time. An
/// event written before positions existed carries `None`.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct ItemTick {
    /// Where the item sat when this was written, if the writer knew.
    #[n(0)]
    pub ord: Option<u32>,
    /// What the item said at the time.
    #[n(1)]
    pub text: String,
    /// Ticked, or put back.
    #[n(2)]
    pub done: bool,
    /// How many items the capture had at the time, where the writer knew.
    ///
    /// Only a position written against the same number of items is comparable;
    /// see [`ItemBody::of`].
    #[n(3)]
    pub of: Option<u32>,
}

/// A tag addition that has not been retracted.
struct LiveTag {
    add: EventId,
    tag: String,
}

/// A name, what it denotes, and when that note came into being.
///
/// The timestamp is not decoration. Links resolve against the names that
/// existed **when the link was written**, so a name is only a candidate if its
/// note is older than the event doing the linking.
pub type Names = BTreeMap<String, (EventId, Hlc)>;

/// Compute state from a set of events.
pub fn fold(events: &[Envelope]) -> Projection {
    fold_seeded(events, &Names::new())
}

/// Fold a subset of the log, given the names the rest of it already
/// established.
///
/// An incremental update folds only the events belonging to the notes that
/// changed. That is enough for text, tags and divergence, all of which depend
/// on a single note's own history — and **not** enough for links, which resolve
/// against every note that existed at the time. A partial fold with no seed
/// resolves every link to nothing, silently, and the note still renders
/// perfectly otherwise.
///
/// So the caller supplies what it already knows. Seeding with names created
/// *after* the linking event would be equally wrong in the other direction —
/// a link would resolve to a note its author could not have seen — which is why
/// each seeded name carries the moment it started existing.
pub fn fold_seeded(events: &[Envelope], known: &Names) -> Projection {
    // Sort by timestamp, not arrival. Everything downstream reads this order,
    // so the result cannot depend on how the events turned up.
    let mut ordered: Vec<&Envelope> = events.iter().collect();
    ordered.sort_by_key(|a| a.header.hlc);

    let mut proj = Projection::default();
    let mut tags: BTreeMap<EventId, Vec<LiveTag>> = BTreeMap::new();
    let mut texts: BTreeMap<EventId, String> = BTreeMap::new();

    // Names as they stood at each point in the fold, and the links each version
    // resolved against them. Both are built during the pass rather than after
    // it, because a link records what a name meant *when it was written* — and
    // once the pass is over, that information is gone.
    let mut names: Names = known.clone();
    let mut version_links: BTreeMap<EventId, Vec<Link>> = BTreeMap::new();

    // Divergences the user has already looked at. Collected during the pass and
    // applied after `resolve`, because a divergence does not exist until every
    // version is known.
    let mut dismissed: BTreeSet<EventId> = BTreeSet::new();
    let mut mutes: Vec<(MuteScope, Option<u64>, Hlc)> = Vec::new();

    // Redactions are collected first and applied last, because a redaction must
    // take effect whether it arrives before or after the thing it destroys. If
    // it were applied in timestamp order, an event that turned up afterwards
    // would simply be readable again — and "destroyed" would mean "destroyed
    // unless something is slow", which is not a guarantee.
    // Every redaction naming a target, not just the first. Which one governs
    // cannot be decided here, because the cancellations are still being read:
    // keeping only the first meant that once a redaction was cancelled, the
    // dead one was the only one on record and every later redaction of that
    // note was silently discarded — content ordered destroyed stayed readable,
    // was never queued, and nothing anywhere said so.
    let mut ordered_redactions: BTreeMap<EventId, Vec<Ordered>> = BTreeMap::new();
    let mut cancelled: BTreeSet<EventId> = BTreeSet::new();

    for e in &ordered {
        match e.header.kind {
            Kind::REDACT => {
                let Some(body) = &e.body else { continue };
                let Ok(r) = minicbor::decode::<RedactBody>(body) else {
                    // A destruction order nobody can read. It used to be
                    // skipped, which made it indistinguishable from no order at
                    // all — and `held` is exactly for an authentic event this
                    // build will not act on.
                    proj.held.insert(e.header.id(), Held::Malformed);
                    continue;
                };
                // Capped. An uncapped redaction lets one loop destroy the whole
                // archive everywhere within a single push round.
                //
                // Over the cap the whole event is malformed and applies to
                // nothing. It used to `take` the first 64 and drop the rest
                // with no record, so a redaction naming a hundred reported
                // success and left thirty-six intact — a redaction of the first
                // 64 is not what anybody asked for, and the cap is a property
                // of the event rather than of each target.
                if r.targets.len() > crate::body::REDACT_TARGET_CAP {
                    proj.held.insert(e.header.id(), Held::Malformed);
                    continue;
                }
                for t in r.targets.iter() {
                    ordered_redactions.entry(*t).or_default().push(Ordered {
                        event: e.header.id(),
                        at: e.header.hlc,
                        window_ms: crate::REDACT_WINDOW_MS,
                        cancelled: false,
                    });
                }
            }
            Kind::REDACT_CANCEL => {
                let Some(body) = &e.body else { continue };
                if let Ok(c) = minicbor::decode::<RedactCancelBody>(body) {
                    cancelled.insert(c.redact);
                }
            }
            _ => {}
        }
    }

    // The earliest redaction still standing governs the note.
    //
    // Earliest rather than latest, for the same reason the window is measured
    // from the redaction's own timestamp: a later order must not grant the
    // content a fresh window and so outlive the earlier one. And still
    // standing, because a cancelled redaction is not an order any more — it
    // must not shadow the ones that came after it.
    //
    // A note whose every redaction has been cancelled keeps the last of them,
    // marked cancelled, so the fold still records that one was asked for.
    let redacted: BTreeMap<EventId, Ordered> = ordered_redactions
        .into_iter()
        .filter_map(|(tgt, mut rs)| {
            let live = rs.iter().find(|r| !cancelled.contains(&r.event)).copied();
            let governing = live.or_else(|| {
                rs.pop().map(|mut r| {
                    r.cancelled = true;
                    r
                })
            })?;
            Some((tgt, governing))
        })
        .collect();

    for e in &ordered {
        if !e.header.kind.is_known() {
            proj.held.insert(e.header.id(), Held::UnknownKind);
            continue;
        }

        match e.header.kind {
            Kind::CAPTURE => apply_capture(&mut proj, &mut names, &mut version_links, e),
            Kind::EDIT => apply_edit(&mut proj, &mut texts, &names, &mut version_links, e),
            Kind::TAG => apply_tag(&mut proj, &mut tags, e),
            Kind::UNTAG => apply_untag(&mut proj, &mut tags, e),
            Kind::PIN => apply_pin(&mut proj, e),
            Kind::PROMOTE => apply_promote(&mut proj, e),
            Kind::REVIEW => apply_review(&mut proj, e),
            Kind::PARENT => apply_parent(&mut proj, e),
            Kind::ENTITY_RENAME => apply_entity_rename(&mut proj, e),
            Kind::ENTITY_MERGE => apply_entity_merge(&mut proj, e),
            Kind::ENTITY_BIND => apply_entity_bind(&mut proj, e),
            Kind::OCC_MOVE => apply_occ_move(&mut proj, e),
            Kind::OCC_EDIT => apply_occ_edit(&mut proj, e),
            Kind::OCC_CANCEL => apply_occ(&mut proj, e, |o| {
                o.cancelled = true;
                o.skipped_last = true;
            }),
            Kind::OCC_DONE => apply_occ(&mut proj, e, |o| o.done = true),
            Kind::ITEM_DONE => apply_item(&mut proj, e, true),
            Kind::ITEM_UNDONE => apply_item(&mut proj, e, false),
            Kind::OCC_UNDONE => apply_occ(&mut proj, e, |o| o.done = false),
            Kind::INTENT => apply_intent(&mut proj, e),
            Kind::ALIAS => apply_alias(&mut proj, &mut names, e),
            Kind::UNFLAG => apply_unflag(&mut proj, e),
            Kind::RESOLVE => collect_resolve(&mut proj, &mut dismissed, e),
            Kind::MUTE => collect_mute(&mut proj, &mut mutes, e),
            Kind::TOMBSTONE => apply_tombstone(&mut proj, e),
            // Membership and settings do not affect note state.
            Kind::SETTING => apply_setting(&mut proj, e),
            // Every membership kind, not only the two that were listed.
            //
            // `ROSTER_KEY`, `DEVICE_RETIRE` and `DEVICE_READMIT` were missing
            // and fell to the arm below, which files an event as
            // `UnknownKind` — meaning "this build has no rule for it, look
            // again after an upgrade". The build has a rule: `roster::build`
            // reads all five outside the fold, which is exactly why the fold
            // has nothing to do with them.
            //
            // It was not an edge case. Every archive mints a roster key at
            // genesis, so *every* projection this build rebuilt held one, and
            // every status surface reported "1 event(s) stored but not folded"
            // for ever. Invisible until a rebuild, which is why it survived:
            // the incremental path does not record held events, so it appeared
            // only after `capture rebuild` or a schema bump.
            //
            // A permanent false positive is worse than saying nothing. §4.5's
            // tiers only work if the list is believable, and one that always
            // has a line in it is one people learn to skip.
            Kind::DEVICE_ADD
            | Kind::ORIGIN_CLAIM
            | Kind::ROSTER_KEY
            | Kind::DEVICE_RETIRE
            | Kind::DEVICE_READMIT => {}
            // Handled outside the main pass.
            Kind::REDACT | Kind::REDACT_CANCEL => {}
            _ => {
                proj.held.insert(e.header.id(), Held::UnknownKind);
            }
        }
    }

    // After every parent edge is in, and before anything reads the tree. Two
    // devices can each write half of a loop without either being wrong at the
    // time, so the loop only exists once both are folded — which is exactly
    // where it can be broken identically on every replica.
    break_parent_cycles(&mut proj);
    break_merge_cycles(&mut proj);

    resolve(&mut proj, &texts);

    // Mutes last: a rule by tag or by date needs every note to be final before
    // it can say which ones it covers.
    apply_mutes(&mut proj, &mutes);

    // A dismissal only clears the versions it named. Sweeping every divergence
    // would silently discard one that arrived after the user looked.
    for n in proj.notes.values_mut() {
        n.divergences.retain(|d| !dismissed.contains(&d.other));
    }

    // A note's links are the links of the version it is showing. Taking them
    // from anywhere else would mean a note displaying one text and linking
    // according to another.
    for n in proj.notes.values_mut() {
        n.links = version_links.get(&n.head).cloned().unwrap_or_default();
    }

    // Redactions, whatever order they arrived in.
    //
    // A redaction is retained rather than applied once: it has to destroy
    // matching events that turn up *after* it, or "destroyed" would mean
    // "destroyed unless something was slow".
    for (tgt, r) in &redacted {
        let Some(n) = proj.notes.get_mut(tgt) else {
            continue;
        };

        // Whether the content is gone is a fact about the log, not a decision
        // taken here: the body either exists or it does not. The fold reports
        // that fact; the purge is what changes it.
        if n.text.is_none() {
            n.redacted_at = Some(r.at);
            // Withdrawn, and too late to matter. Kept because nothing else
            // knows both halves at once.
            if r.cancelled {
                n.cancelled_too_late = Some(r.event);
            }
            // Rebuilt from identity alone. The note stays reconstructible on
            // every future fold, and its name stops carrying the words it was
            // built from — which otherwise propagate into every note linking
            // here, where no redaction can reach them.
            n.slug = slug::derive_redacted(tgt).text;
        } else if !r.cancelled {
            n.redaction_pending = Some(Pending {
                redact: r.event,
                execute_after: r.at.wall.saturating_add(r.window_ms),
            });
        }
    }

    proj
}

fn apply_capture(
    proj: &mut Projection,
    names: &mut Names,
    version_links: &mut BTreeMap<EventId, Vec<Link>>,
    e: &Envelope,
) {
    let tgt = e.header.tgt.unwrap_or_else(|| e.header.id());

    // Three cases, and only one of them is a fault.
    //
    // A capture whose body is **gone** still makes a note. Its content was
    // destroyed — by a redaction, or stripped on the way into an export — and
    // the note must survive that, or the archive would lose whole entries every
    // time the projection was rebuilt rather than losing only their text. A
    // note that renders as "no readable text" is a fact; a note that silently
    // stops existing is data loss.
    //
    // A body that is present and **unreadable** is different: this build cannot
    // interpret it, which is a real fault and is held as one.
    let (text, tz, ambient) = match e.body.as_ref().map(|b| minicbor::decode::<CaptureBody>(b)) {
        Some(Ok(c)) => (Some(c.text), c.tz, c.ambient),
        None => (None, None, crate::body::Ambient::default()),
        Some(Err(_)) => {
            proj.held.insert(e.header.id(), Held::Malformed);
            return;
        }
    };

    // The name derives from the capture, which never changes — not from the
    // current text, which does. Otherwise an edit would rename the note and
    // break every link written to it.
    //
    // With no text there is nothing to derive from, so the name comes from
    // identity alone — the same name a redaction leaves behind, and for the
    // same reason: a name built from destroyed words would still be carrying
    // them.
    // The capture's own moment, not a placeholder. This read `1970-01-01`
    // until a surface existed that could make a textless capture — nothing
    // could reach it, so every image and every empty capture would have been
    // named for the epoch the first time one was written.
    //
    // UTC rather than the capture's zone, deliberately: see `slug::utc_date`.
    // A permanent name must not depend on which timezone database folded it.
    let date = slug::utc_date(e.header.hlc.wall as i64);
    let s = match text.as_deref() {
        Some(t) => slug::derive(t, &date, &tgt),
        None => slug::derive_redacted(&tgt),
    };

    proj.notes.insert(
        tgt,
        Note {
            tgt,
            slug: s.text,
            alias: None,
            alias_at: None,
            parent: None,
            parent_at: None,
            credential: crate::slug::looks_like_credential(text.as_deref().unwrap_or("")),
            text,
            ambient,
            head: e.header.id(),
            created: e.header.hlc,
            updated: e.header.hlc,
            tz,
            tags: BTreeSet::new(),
            tag_adds: Vec::new(),
            pinned: false,
            promotion: None,
            by_person: None,
            by_parser: None,
            review: ReviewState::default(),
            occurrences: BTreeMap::new(),
            item_ticks: Vec::new(),
            muted_until: None,
            muted_at: None,
            redacted_at: None,
            redaction_pending: None,
            cancelled_too_late: None,
            divergences: Vec::new(),
            links: Vec::new(),
            versions: alloc::vec![Version {
                id: e.header.id(),
                prev: None,
                hlc: e.header.hlc,
                kind: Kind::CAPTURE,
                excluded: false,
            }],
        },
    );

    // Registered before its own links are read, so a note that links itself
    // resolves rather than dangling.
    if let Some(n) = proj.notes.get(&tgt) {
        names.insert(n.slug.clone(), (tgt, e.header.hlc));
        let text = n.text.clone().unwrap_or_default();
        version_links.insert(e.header.id(), resolve_links(&text, names, e.header.hlc));
    }
}

/// Read the links out of a text and bind each to what its name meant then.
///
/// `at` is the moment the text was written. A name whose note is younger than
/// that is not a candidate: the author could not have meant it.
fn resolve_links(text: &str, names: &Names, at: Hlc) -> Vec<Link> {
    link::extract(text)
        .into_iter()
        .map(|name| {
            let resolved = names
                .get(&name)
                .filter(|(_, born)| *born <= at)
                .map(|(id, _)| *id);
            Link { name, resolved }
        })
        .collect()
}

fn apply_edit(
    proj: &mut Projection,
    texts: &mut BTreeMap<EventId, String>,
    names: &Names,
    version_links: &mut BTreeMap<EventId, Vec<Link>>,
    e: &Envelope,
) {
    let Some(tgt) = e.header.tgt else {
        proj.held.insert(e.header.id(), Held::NoTarget);
        return;
    };
    let Some(note) = proj.notes.get_mut(&tgt) else {
        proj.held.insert(e.header.id(), Held::NoTarget);
        return;
    };
    let text = match e.body.as_ref().map(|b| minicbor::decode::<EditBody>(b)) {
        Some(Ok(v)) => v.text,
        _ => {
            proj.held.insert(e.header.id(), Held::Malformed);
            return;
        }
    };

    // Record the version and move on. Which version *wins*, and whether any two
    // were authored independently, is decided in one pass once every version is
    // known — see `resolve`.
    //
    // Deciding incrementally looks natural and is wrong: a divergence between
    // two versions can stop being a divergence when a later edit supersedes one
    // of them, and an incremental rule has already recorded it by then.
    note.versions.push(Version {
        id: e.header.id(),
        prev: e.header.prev,
        hlc: e.header.hlc,
        kind: Kind::EDIT,
        excluded: false,
    });
    version_links.insert(e.header.id(), resolve_links(&text, names, e.header.hlc));
    texts.insert(e.header.id(), text);
}

/// Decide each note's current version, and which versions were independent.
///
/// A version is a **branch tip** if nothing supersedes it. One tip means a
/// single line of edits. More than one means the note was edited in more than
/// one place without those edits seeing each other, and every tip is a version
/// somebody wrote and nobody replaced.
///
/// Computing this from the finished graph rather than as events arrive is what
/// makes the answer independent of order — the set of tips is a property of the
/// graph, and the graph is the same however it was assembled.
fn resolve(proj: &mut Projection, texts: &BTreeMap<EventId, String>) {
    for note in proj.notes.values_mut() {
        let superseded: BTreeSet<EventId> = note
            .versions
            .iter()
            .filter(|v| !v.excluded)
            .filter_map(|v| v.prev)
            .collect();

        let mut tips: Vec<&Version> = note
            .versions
            .iter()
            .filter(|v| !v.excluded && !superseded.contains(&v.id))
            .collect();

        if tips.is_empty() {
            continue;
        }

        // Later timestamp is shown. The tiebreak is total, so every device
        // picks the same one.
        tips.sort_by_key(|v| v.hlc);
        let winner = tips[tips.len() - 1];

        note.head = winner.id;
        note.updated = winner.hlc;
        if let Some(t) = texts.get(&winner.id) {
            note.text = Some(t.clone());
        }

        // Every tip that lost is kept and reported. Losing an edit outright is
        // worse than showing the wrong one: the wrong one is visible and can be
        // corrected, a lost one leaves no trace that anything happened.
        note.divergences = tips[..tips.len() - 1]
            .iter()
            .map(|t| Divergence {
                current: winner.id,
                other: t.id,
            })
            .collect();
        note.divergences.sort();
    }
}

/// Whether `from` reaches `to` by following predecessors.
///
/// This is the primitive the specification describes for telling a sequential
/// edit from an independent one, and the fold no longer calls it — branch tips
/// answer the same question more directly.
///
/// The two are equivalent for that question, and the equivalence is worth
/// stating because it is not obvious: if one version reaches another, then
/// something names that other version as its predecessor, so the other is not
/// a tip. Two distinct tips are therefore always mutually unreachable. It is
/// kept, and tested against the tip rule, so that the claim is checked rather
/// than asserted.
pub fn reaches(versions: &[Version], from: EventId, to: EventId) -> bool {
    let mut at = Some(from);
    let mut steps = 0usize;
    while let Some(id) = at {
        if id == to {
            return true;
        }
        // Bounded: a cycle would otherwise hang the fold, and a malformed
        // predecessor chain is exactly the kind of input this must survive.
        steps += 1;
        if steps > versions.len() + 1 {
            return false;
        }
        at = versions.iter().find(|v| v.id == id).and_then(|v| v.prev);
    }
    false
}

fn apply_tag(proj: &mut Projection, tags: &mut BTreeMap<EventId, Vec<LiveTag>>, e: &Envelope) {
    let Some(tgt) = e.header.tgt else {
        proj.held.insert(e.header.id(), Held::NoTarget);
        return;
    };
    if !proj.notes.contains_key(&tgt) {
        proj.held.insert(e.header.id(), Held::NoTarget);
        return;
    }
    let tag = match e.body.as_ref().map(|b| minicbor::decode::<TagBody>(b)) {
        Some(Ok(v)) => v.tag,
        _ => {
            proj.held.insert(e.header.id(), Held::Malformed);
            return;
        }
    };
    tags.entry(tgt).or_default().push(LiveTag {
        add: e.header.id(),
        tag: tag.clone(),
    });
    if let Some(n) = proj.notes.get_mut(&tgt) {
        n.tags.insert(tag.clone());
        n.tag_adds.push((tag, e.header.id()));
    }
}

fn apply_untag(proj: &mut Projection, tags: &mut BTreeMap<EventId, Vec<LiveTag>>, e: &Envelope) {
    let Some(tgt) = e.header.tgt else {
        proj.held.insert(e.header.id(), Held::NoTarget);
        return;
    };
    let kills = match e.body.as_ref().map(|b| minicbor::decode::<UntagBody>(b)) {
        Some(Ok(v)) => v.kills,
        _ => {
            proj.held.insert(e.header.id(), Held::Malformed);
            return;
        }
    };

    // Retract only the additions this device actually saw. An addition made
    // elsewhere, which this removal could not have known about, survives — so
    // two devices that act independently still agree afterwards.
    let Some(live) = tags.get_mut(&tgt) else {
        return;
    };
    live.retain(|t| !kills.contains(&t.add));

    if let Some(n) = proj.notes.get_mut(&tgt) {
        n.tags = live.iter().map(|t| t.tag.clone()).collect();
        n.tag_adds = live.iter().map(|t| (t.tag.clone(), t.add)).collect();
    }
}

/// Record a chosen preference.
///
/// Last writer wins, and events are already in timestamp order, so the newest
/// choice is simply the last one applied. A revert is stored as a choice with
/// no value rather than by deleting the row: "I decided to go back to the
/// default" and "I never decided" are different facts, and only the second one
/// should follow a future change to what the default is.
fn apply_setting(proj: &mut Projection, e: &Envelope) {
    let Some(body) = e.body.as_ref() else { return };
    let Ok(v) = minicbor::decode::<SettingBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    proj.settings.insert(
        v.key,
        Setting {
            value: v.value,
            set_by: e.header.dev,
            at: e.header.hlc,
        },
    );
}

/// Clear a note's credential flag.
///
/// One direction only. Setting the flag is the predicate's job at capture and
/// nothing can turn it back on, because a note somebody has already declared
/// safe should not become unbackupable again on the next rebuild.
fn apply_unflag(proj: &mut Projection, e: &Envelope) {
    let Some(tgt) = e.header.tgt else {
        proj.held.insert(e.header.id(), Held::NoTarget);
        return;
    };
    if let Some(n) = proj.notes.get_mut(&tgt) {
        n.credential = false;
    } else {
        proj.held.insert(e.header.id(), Held::NoTarget);
    }
}

/// Assign a derived type to a note.
///
/// # Authority before recency, and only here
///
/// Every other last-write-wins field in this fold compares timestamps and stops.
/// This one asks who wrote it first, and it is the only rule in the system that
/// does.
///
/// The reason is a specific failure. The parser will improve, and reparsing the
/// backlog is desirable — failure mode 20b says so. Under plain last-write-wins
/// a reparse would carry a later HLC than the correction somebody made by hand
/// last month, and would silently revert it. A tool that quietly undoes your
/// corrections is one you stop trusting, and §6 rates that unrecoverable
/// against the cost of having one extra rule to remember.
///
/// So: a pinned promotion is never displaced by an unpinned one. Among
/// promotions of equal authority, the later by HLC wins as everywhere else.
///
/// # And a person can take their word back
///
/// Found in use on 2026-10-13: a suggestion accepted by mistake pinned the
/// wrong day, the text was then corrected, and nothing moved — nor could
/// anything, since nothing outranks a pin. A *release* is a person's promotion
/// that is not a pin (`PromoteBody::released`): it carries a parser's reading
/// of the text, and it is the person's latest word, so it displaces their
/// earlier correction. After it, parser promotions compete with it by plain
/// recency, so the next edit re-promotes as if no correction had been made.
///
/// Kept as two last-write-wins registers — the person's latest and the
/// parser's latest — with the promotion derived from both, so the answer is
/// the same in every arrival order: a pin wins outright; a release competes
/// with the parser by HLC; with neither, the parser's stands.
fn apply_promote(proj: &mut Projection, e: &Envelope) {
    let Some(tgt) = e.header.tgt else { return };
    let Some(body) = e.body.as_ref() else { return };
    let Ok(p) = minicbor::decode::<PromoteBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    let Some(n) = proj.notes.get_mut(&tgt) else {
        return;
    };

    let incoming = Promotion {
        as_: p.as_,
        pinned: p.pinned,
        grammar: p.grammar,
        confidence: p.confidence,
        at: e.header.hlc,
        event: e.header.id(),
    };

    let person = p.pinned || p.released == Some(true);
    let register = if person {
        &mut n.by_person
    } else {
        &mut n.by_parser
    };
    if register.as_ref().is_none_or(|c| incoming.at > c.at) {
        *register = Some(incoming);
    }

    n.promotion = match (&n.by_person, &n.by_parser) {
        // A human correction stands, however late the machine is.
        (Some(pin), _) if pin.pinned => Some(pin.clone()),
        // A release, then: the person's last word, until the parser's is later.
        (Some(released), Some(parsed)) if parsed.at > released.at => Some(parsed.clone()),
        (Some(released), _) => Some(released.clone()),
        (None, parsed) => parsed.clone(),
    };
}

/// Record an if-then plan.
fn apply_intent(proj: &mut Projection, e: &Envelope) {
    let Some(body) = e.body.as_ref() else { return };
    let Ok(i) = minicbor::decode::<crate::body::IntentBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    proj.intents.insert(
        e.header.id(),
        Intent {
            cue: i.cue,
            then: i.then,
            at: e.header.hlc,
            retracted: false,
        },
    );
}

/// Record that a note was resurfaced, and what became of it.
///
/// Three outcomes and no fourth. `recalled / missed / snoozed` was withdrawn in
/// §2.7, and the encoding has no representation for it — which is why this can
/// only ever count what somebody did rather than how well they remembered.
fn apply_review(proj: &mut Projection, e: &Envelope) {
    let Some(tgt) = e.header.tgt else { return };
    let Some(body) = e.body.as_ref() else { return };
    let Ok(r) = minicbor::decode::<ReviewBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    let Some(n) = proj.notes.get_mut(&tgt) else {
        return;
    };

    // Later wins for the timestamp, and the counts accumulate. A union rather
    // than last-write-wins: two devices offering the same note independently
    // both happened, and picking one would lose the other.
    if n.review.last_shown.is_none_or(|at| e.header.hlc > at) {
        n.review.last_shown = Some(e.header.hlc);
    }
    match r.outcome {
        Outcome::Engaged => n.review.engaged += 1,
        Outcome::Deferred => n.review.deferred += 1,
        Outcome::Shown => {}
    }
}

/// Put a note under another, or back at the top.
///
/// Ordinary last-write-wins. `None` is a decision rather than an absence —
/// "no longer under anything" has to be able to beat an earlier parent — and
/// the kind is what says so, since a `parent` event with no parent in it can
/// mean nothing else.
fn apply_parent(proj: &mut Projection, e: &Envelope) {
    let Some(tgt) = e.header.tgt else { return };
    let Some(body) = e.body.as_ref() else { return };
    let Ok(p) = minicbor::decode::<ParentBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    let Some(n) = proj.notes.get_mut(&tgt) else {
        return;
    };
    if n.parent_at.is_none_or(|at| e.header.hlc > at) {
        // A note under itself is refused where it is written, rather than left
        // for the cycle pass: it is the one loop somebody can make on purpose,
        // and it has no reading at all.
        n.parent = p.parent.filter(|q| *q != tgt);
        n.parent_at = Some(e.header.hlc);
    }
}

/// Break any loop in the tree, identically everywhere.
///
/// Two devices can each file a page under the other without either being wrong
/// when it happens: A under B is a fine thing to say, and so is B under A, and
/// only the pair is a problem. So the loop cannot be refused at the point of
/// writing — it does not exist yet — and has to be resolved after the fold.
///
/// The newest edge in the loop is the one dropped, by HLC and then by note id
/// so the answer is total. That leaves the older arrangement standing, which is
/// the same instinct as the rest of the conflict rules here: the surprising
/// outcome should be the recent one, because it is the one somebody still
/// remembers making.
fn break_parent_cycles(proj: &mut Projection) {
    let ids: Vec<EventId> = proj.notes.keys().copied().collect();

    for start in ids {
        // Walk up from this note. A chain longer than the number of notes has
        // to have repeated one, and the bound is also what stops a malformed
        // set of edges spinning here for ever.
        let mut seen: Vec<EventId> = Vec::new();
        let mut at = start;
        loop {
            if seen.contains(&at) {
                break;
            }
            seen.push(at);
            let Some(next) = proj.notes.get(&at).and_then(|n| n.parent) else {
                // Reached the top: no loop on this path.
                seen.clear();
                break;
            };
            if !proj.notes.contains_key(&next) {
                // A parent that is not here — a forward reference, or a note
                // this replica does not hold. Not a loop, and not an error: the
                // tree draws it at the top until the parent arrives.
                seen.clear();
                break;
            }
            at = next;
        }

        if seen.is_empty() {
            continue;
        }

        // `at` is the note the walk came back to, so the loop is the tail of
        // `seen` from there. Drop its newest edge.
        let Some(from) = seen.iter().position(|x| *x == at) else {
            continue;
        };
        let loop_members = &seen[from..];
        let Some(newest) = loop_members
            .iter()
            .max_by_key(|id| {
                let n = &proj.notes[*id];
                (n.parent_at, **id)
            })
            .copied()
        else {
            continue;
        };
        if let Some(n) = proj.notes.get_mut(&newest) {
            n.parent = None;
        }
    }
}

/// Give an entity a name somebody chose.
///
/// Ordinary last-write-wins on the display, and nothing else. Identity is a
/// function of the written spelling (see [`EntId`]), so a rename cannot move an
/// entity — which is the whole reason §4.3 wanted an id in the first place.
///
/// An empty name is refused rather than stored. A page with no name is one
/// nothing can point at, and clearing a name is not a thing this event means:
/// the fallback is the spelling somebody wrote, which is always there.
fn apply_entity_rename(proj: &mut Projection, e: &Envelope) {
    let Some(body) = e.body.as_ref() else { return };
    let Ok(r) = minicbor::decode::<EntityRenameBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    if r.display.trim().is_empty() {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    }
    match proj.entity_display.get(&r.ent) {
        Some((_, at)) if *at >= e.header.hlc => {}
        _ => {
            proj.entity_display.insert(r.ent, (r.display, e.header.hlc));
        }
    }
}

/// Record that two entities were one.
///
/// Stored as an edge rather than applied, so a chain stays a chain and
/// resolution walks it. Merging an entity into itself is dropped: it says
/// nothing and would be a one-element cycle for the pass below to clean up.
fn apply_entity_merge(proj: &mut Projection, e: &Envelope) {
    let Some(body) = e.body.as_ref() else { return };
    let Ok(m) = minicbor::decode::<EntityMergeBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    if m.from == m.into {
        return;
    }
    match proj.entity_merged.get(&m.from) {
        Some((_, at)) if *at >= e.header.hlc => {}
        _ => {
            proj.entity_merged.insert(m.from, (m.into, e.header.hlc));
        }
    }
}

/// Bind the note that *is* this entity, or unbind it.
///
/// A bind naming a note this replica does not hold is held rather than applied,
/// as `apply_alias` does for the same situation: the note may simply not have
/// arrived yet, and guessing would bind the page to nothing.
///
/// Unbinding carries `None` and is applied, not skipped — "backed by nothing"
/// has to be able to beat an earlier bind.
fn apply_entity_bind(proj: &mut Projection, e: &Envelope) {
    let Some(body) = e.body.as_ref() else { return };
    let Ok(b) = minicbor::decode::<EntityBindBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    if let Some(note) = b.note
        && !proj.notes.contains_key(&note)
    {
        proj.held.insert(e.header.id(), Held::NoTarget);
        return;
    }
    match proj.entity_backing.get(&b.ent) {
        Some((_, at)) if *at >= e.header.hlc => {}
        _ => {
            proj.entity_backing.insert(b.ent, (b.note, e.header.hlc));
        }
    }
}

/// Break merge cycles, so resolution terminates and no entity is lost.
///
/// Two devices can merge A into B and B into A while partitioned, and neither is
/// wrong from where it stands. `break_parent_cycles` already solves this exact
/// shape and this is deliberately the same: find the loop, drop its newest edge
/// by `(at, id)`, and do it identically on every device so both agree.
///
/// Dropping the newest rather than refusing the merge matters. A refusal would
/// leave both entities pointing at each other, and every read that followed the
/// pointer would either spin or give up — so an entity somebody merged would
/// become unreachable, which is losing it.
fn break_merge_cycles(proj: &mut Projection) {
    let ids: Vec<EntId> = proj.entity_merged.keys().copied().collect();

    for start in ids {
        // A chain longer than the number of edges has to have repeated one, and
        // the bound is what stops a malformed set spinning here for ever.
        let mut seen: Vec<EntId> = Vec::new();
        let mut at = start;
        loop {
            if seen.contains(&at) {
                break;
            }
            seen.push(at);
            let Some((next, _)) = proj.entity_merged.get(&at) else {
                // Reached an entity that is nobody else's: no loop this way.
                seen.clear();
                break;
            };
            at = *next;
        }

        if seen.is_empty() {
            continue;
        }

        let Some(from) = seen.iter().position(|x| *x == at) else {
            continue;
        };
        let loop_members = &seen[from..];
        let Some(newest) = loop_members
            .iter()
            .max_by_key(|id| {
                let (_, at) = proj.entity_merged[*id];
                (at, **id)
            })
            .copied()
        else {
            continue;
        };
        proj.entity_merged.remove(&newest);
    }
}

/// Move one occurrence, leaving the rule alone.
fn apply_occ_move(proj: &mut Projection, e: &Envelope) {
    let Some(tgt) = e.header.tgt else { return };
    let Some(body) = e.body.as_ref() else { return };
    let Ok(m) = minicbor::decode::<OccMoveBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    if let Some(n) = proj.notes.get_mut(&tgt) {
        let o = n.occurrences.entry(m.occ).or_default();
        o.moved_to = Some((m.start_ms, m.end_ms));
        o.skipped_last = false;
    }
}

/// Change one occurrence's details, leaving the rule alone.
fn apply_occ_edit(proj: &mut Projection, e: &Envelope) {
    let Some(tgt) = e.header.tgt else { return };
    let Some(body) = e.body.as_ref() else { return };
    let Ok(ed) = minicbor::decode::<OccEditBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    if let Some(n) = proj.notes.get_mut(&tgt) {
        let o = n.occurrences.entry(ed.occ).or_default();
        // Absent means "unchanged", not "cleared". Two devices editing
        // different details of one occurrence must not erase each other.
        if ed.title.is_some() {
            o.title = ed.title;
        }
        if ed.location.is_some() {
            o.location = ed.location;
        }
        if ed.notes.is_some() {
            o.notes = ed.notes;
        }
    }
}

/// Cancel, complete or un-complete one occurrence.
///
/// The three share a body shape and differ only by kind, so they share this.
/// Completing twice is completing once — the flag is set, not counted — and an
/// `occ_undone` clears it, because un-ticking a mistaken completion has to be
/// possible and a pure union could not express that (§6).
fn apply_occ(proj: &mut Projection, e: &Envelope, set: impl Fn(&mut OccState)) {
    let Some(tgt) = e.header.tgt else { return };
    let Some(body) = e.body.as_ref() else { return };
    let Ok(o) = minicbor::decode::<OccBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    if let Some(n) = proj.notes.get_mut(&tgt) {
        set(n.occurrences.entry(o.occ).or_default());
    }
}

/// Tick one item of a list capture off, or put it back.
///
/// The list's twin of [`apply_occ`], and deliberately the same shape. An item
/// is a projection row expanded from this capture's text; what is recorded
/// here is a fact about that capture, and nothing new is created.
///
/// The item is not checked against the capture's current text. A list edited
/// to drop an item leaves the tick recorded against a name nothing expands
/// any more, which is inert — and the alternative, discarding it, would mean
/// an edit destroyed a record of something that actually happened. Restoring
/// the item restores the tick, which is the behaviour somebody who edited by
/// mistake would want.
fn apply_item(proj: &mut Projection, e: &Envelope, done: bool) {
    let Some(tgt) = e.header.tgt else { return };
    let Some(body) = e.body.as_ref() else { return };
    let Ok(i) = minicbor::decode::<ItemBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    if let Some(n) = proj.notes.get_mut(&tgt) {
        // Appended, not resolved. Which item this meant is a question about the
        // capture's current text, and answering it here would need a parser
        // this crate cannot reach. Events arrive in timestamp order, so the
        // order of this list is already the order the projection should weigh
        // them in.
        n.item_ticks.push(ItemTick {
            ord: i.ord,
            text: i.item,
            done,
            of: i.of,
        });
    }
}

fn apply_pin(proj: &mut Projection, e: &Envelope) {
    let Some(tgt) = e.header.tgt else { return };
    let Some(body) = e.body.as_ref() else { return };
    let Ok(p) = minicbor::decode::<PinBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    if let Some(n) = proj.notes.get_mut(&tgt) {
        // A plain value with no history to reconcile: the later one wins, and
        // events are already in timestamp order.
        n.pinned = p.pinned;
    }
}

/// Assign or clear a note's chosen name.
///
/// Applied here, in the middle of the pass, rather than afterwards. Links
/// resolve as the pass walks the events, so an alias decided at the end would
/// be invisible to every link written against it — the note would have the
/// name and nothing would point at it.
///
/// One namespace: a name that is already a slug or another note's alias is
/// **declined**. Declining rather than overwriting is what makes a name mean
/// one thing, and doing it in timestamp order means every replica declines the
/// same assignment.
fn apply_alias(proj: &mut Projection, names: &mut Names, e: &Envelope) {
    let Some(tgt) = e.header.tgt else {
        proj.held.insert(e.header.id(), Held::NoTarget);
        return;
    };
    let Some(body) = e.body.as_ref() else { return };
    let Ok(a) = minicbor::decode::<AliasBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    if !proj.notes.contains_key(&tgt) {
        proj.held.insert(e.header.id(), Held::NoTarget);
        return;
    }

    // Released first, so a note can be renamed to something and back without
    // colliding with itself.
    if let Some(n) = proj.notes.get_mut(&tgt)
        && let Some(old) = n.alias.take()
    {
        n.alias_at = None;
        names.remove(&old);
    }

    let Some(name) = a.alias else { return };

    // Taken only if *another* note holds it. Checking mere presence looks
    // equivalent and is not: an incremental fold is seeded with the names the
    // projection already has, which includes this note's own alias, so a
    // presence check makes the note collide with itself and quietly drop the
    // name every time anything else about it changes.
    if names.get(&name).is_some_and(|(holder, _)| *holder != tgt) {
        return;
    }
    if let Some(n) = proj.notes.get_mut(&tgt) {
        n.alias = Some(name.clone());
        n.alias_at = Some(e.header.hlc);
        names.insert(name, (tgt, e.header.hlc));
    }
}

fn collect_resolve(proj: &mut Projection, dismissed: &mut BTreeSet<EventId>, e: &Envelope) {
    let Some(body) = e.body.as_ref() else { return };
    let Ok(r) = minicbor::decode::<ResolveBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    dismissed.extend(r.dismiss);
}

/// Collect a mute. It is applied once every note is final.
///
/// Deferred rather than applied here because a mute by tag or by date has to
/// know things the pass does not know yet: which notes carry the tag after
/// every tag and untag, and what day each note belongs to. Applying it as it
/// arrives would mute whatever happened to exist at that moment and silently
/// miss anything that qualified later.
fn collect_mute(
    proj: &mut Projection,
    mutes: &mut Vec<(MuteScope, Option<u64>, Hlc)>,
    e: &Envelope,
) {
    let Some(body) = e.body.as_ref() else { return };
    let Ok(m) = minicbor::decode::<MuteBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    mutes.push((m.scope, m.until, e.header.hlc));
}

/// Work out which notes are suppressed.
///
/// Applied after everything else, in timestamp order, so the newest mute
/// touching a note wins. A mute is not a property of the note it was written
/// against — it is a rule, and the notes it covers are whichever ones match
/// when the question is asked.
fn apply_mutes(proj: &mut Projection, mutes: &[(MuteScope, Option<u64>, Hlc)]) {
    for (scope, until, at) in mutes {
        // Zero means indefinite, which is distinct from absent: a note muted
        // forever and a note that was never muted look the same otherwise.
        let value = Some(until.unwrap_or(0));

        match scope {
            MuteScope::Note(tgt) => {
                if let Some(n) = proj.notes.get_mut(tgt) {
                    n.muted_until = value;
                    n.muted_at = Some(*at);
                }
            }
            MuteScope::Tag(tag) => {
                for n in proj.notes.values_mut() {
                    if n.tags.contains(tag) {
                        n.muted_until = value;
                        n.muted_at = Some(*at);
                    }
                }
                // And on, for whichever notes wrote `#tag` in their text. See
                // `Projection::tag_mutes`.
                proj.tag_mutes.push((tag.clone(), value, *at));
            }
            MuteScope::DateRange(from, to) => {
                // Floating dates, compared against the day the note was
                // captured in *its own* timezone. Comparing against the
                // reader's would move notes in and out of a mute depending on
                // where they were read.
                for n in proj.notes.values_mut() {
                    let day = capture_day(n);
                    if day.as_str() >= from.as_str() && day.as_str() <= to.as_str() {
                        n.muted_until = value;
                        n.muted_at = Some(*at);
                    }
                }
            }
            // Left unresolved here, and it is the one scope that cannot be
            // settled in this crate.
            //
            // Which notes name an entity is a question about their text, and
            // answering it needs `capture_parse::mentions` — which depends on
            // this crate, so this crate cannot call it without a cycle. The
            // rule is therefore carried out whole, with the timestamp that
            // decides it, and applied where mentions are already computed.
            //
            // `muted_at` above exists for that hand-off. Without it a mute
            // resolved later would beat every note-scoped mute regardless of
            // when either was written, because newest-wins here is iteration
            // order rather than a comparison.
            MuteScope::Entity(ent) => {
                proj.entity_mutes.push((*ent, value, *at));
            }
        }
    }
}

/// The UTC calendar day a note was captured on.
///
/// **In UTC, not in the note's own timezone**, and that is a real limitation
/// rather than an oversight. `capture-core` holds no timezone database and must
/// not — it would be a table that changes under political decisions, hashed
/// into nothing but reachable from the fold, which has to give the same answer
/// on every replica forever.
///
/// The consequence is bounded and worth stating: a note captured late at night
/// west of UTC, or early in the morning east of it, can fall on the
/// neighbouring day at the *edges* of a muted range. A date-range mute is a
/// coarse instrument — "quiet everything from the conference" — so being off by
/// a day at a boundary suppresses one note too many or too few, and nothing is
/// lost either way.
///
/// The timeline groups by the note's own zone, because that is a display
/// decision made where a timezone database exists. This is a fold decision,
/// which is a different constraint.
fn capture_day(n: &Note) -> String {
    let days = (n.created.wall as i64).div_euclid(86_400_000);
    civil_from_days(days)
}

/// Convert days since the epoch to `YYYY-MM-DD`.
fn civil_from_days(z: i64) -> alloc::string::String {
    // Howard Hinnant's civil-from-days, which needs no tables.
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    alloc::format!("{y:04}-{m:02}-{d:02}")
}

fn apply_tombstone(proj: &mut Projection, e: &Envelope) {
    let Some(tgt) = e.header.tgt else { return };

    // Abandoning a plan, which is a different thing from deleting a note and
    // is allowed to simply work. §2.4 rates being easy to walk away from as
    // part of the case for having intents at all.
    if let Some(intent) = proj.intents.get_mut(&tgt) {
        intent.retracted = true;
        return;
    }

    let Some(note) = proj.notes.get_mut(&tgt) else {
        proj.held.insert(e.header.id(), Held::NoTarget);
        return;
    };
    // Recorded, and does not take effect: an edit made independently of a
    // deletion keeps the note. Showing a note someone meant to delete is a
    // visible mistake they can repeat; discarding an edit is not.
    note.versions.push(Version {
        id: e.header.id(),
        prev: e.header.prev,
        hlc: e.header.hlc,
        kind: Kind::TOMBSTONE,
        excluded: true,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;

    /// Membership is not note state, and is not an unknown kind either.
    ///
    /// Every archive mints a roster key at genesis, so this is not an edge
    /// case: before the fix, *every* projection rebuilt by this build held that
    /// event as `UnknownKind` and every status surface said "1 event(s) stored
    /// but not folded — this build has no rule for them". The build knows
    /// exactly what a roster key is; `roster::build` reads these kinds outside
    /// the fold, which is why the fold has nothing to do with them.
    ///
    /// A permanent false positive is worse than silence: §4.5's two tiers only
    /// work if the list is believable, and one that always has a line in it is
    /// one people learn to skip.
    #[test]
    fn membership_kinds_are_ignored_rather_than_held() {
        for kind in [
            Kind::DEVICE_ADD,
            Kind::ORIGIN_CLAIM,
            Kind::ROSTER_KEY,
            Kind::DEVICE_RETIRE,
            Kind::DEVICE_READMIT,
        ] {
            let mut d = testing::Origin::new(0);
            let e = d.author(kind, None, None, alloc::vec![0u8; 16], 1_700_000_000_000);
            let proj = fold(core::slice::from_ref(&e));
            assert!(
                proj.held.is_empty(),
                "kind {kind:?} was held, and membership is read outside the fold",
            );
        }
    }

    /// Two notes, where the second links the first by name.
    fn linked_pair(second_text: &str) -> (Projection, EventId, EventId) {
        let mut d = testing::Origin::new(0);
        let first = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "budget planning for next quarter"),
            1_700_000_000_000,
        );
        let first_id = first.header.id();
        let slug = fold(core::slice::from_ref(&first)).notes[&first_id]
            .slug
            .clone();

        let second = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(2, &second_text.replace("{}", &slug)),
            1_700_000_001_000,
        );
        let second_id = second.header.id();
        (fold(&[first, second]), first_id, second_id)
    }

    #[test]
    fn a_link_resolves_to_the_note_its_name_meant() {
        let (p, first, second) = linked_pair("see [[{}]] for the figures");
        let links = &p.notes[&second].links;
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].resolved, Some(first));
    }

    #[test]
    fn a_link_keeps_the_name_as_written() {
        // Without it a broken link cannot say what it was reaching for.
        let (p, first, second) = linked_pair("see [[{}]] for the figures");
        assert_eq!(p.notes[&second].links[0].name, p.notes[&first].slug);
    }

    #[test]
    fn a_link_to_a_name_that_did_not_exist_resolves_to_nothing() {
        // A forward reference is an ordinary way to write, not an error. It
        // simply has no authoring-time target to be compared against later.
        let (p, _first, second) = linked_pair("see [[not-a-note-abc123]] later");
        assert_eq!(p.notes[&second].links[0].resolved, None);
    }

    #[test]
    fn links_belong_to_the_version_being_shown() {
        // A note displaying one text while linking according to another would
        // be the projection disagreeing with itself.
        let mut d = testing::Origin::new(0);
        let target = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "budget planning for next quarter"),
            1_700_000_000_000,
        );
        let target_id = target.header.id();
        let slug = fold(core::slice::from_ref(&target)).notes[&target_id]
            .slug
            .clone();

        let note = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(2, &alloc::format!("first draft, see [[{slug}]]")),
            1_700_000_001_000,
        );
        let note_id = note.header.id();
        let edit = d.author(
            Kind::EDIT,
            Some(note_id),
            Some(note_id),
            testing::edit_body(3, "rewritten, no links at all"),
            1_700_000_002_000,
        );

        let p = fold(&[target, note, edit]);
        assert!(
            p.notes[&note_id].links.is_empty(),
            "the superseded version's links must not survive the edit"
        );
    }

    #[test]
    fn an_edit_can_add_a_link() {
        let mut d = testing::Origin::new(0);
        let target = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "budget planning for next quarter"),
            1_700_000_000_000,
        );
        let target_id = target.header.id();
        let slug = fold(core::slice::from_ref(&target)).notes[&target_id]
            .slug
            .clone();

        let note = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(2, "nothing here yet"),
            1_700_000_001_000,
        );
        let note_id = note.header.id();
        let edit = d.author(
            Kind::EDIT,
            Some(note_id),
            Some(note_id),
            testing::edit_body(3, &alloc::format!("now see [[{slug}]]")),
            1_700_000_002_000,
        );

        let p = fold(&[target, note, edit]);
        assert_eq!(p.notes[&note_id].links[0].resolved, Some(target_id));
    }

    #[test]
    fn link_resolution_does_not_depend_on_arrival_order() {
        // Resolution reads the names that existed at authoring time, and
        // authoring time comes from the timestamp — not from when the event
        // turned up. Otherwise two replicas would disagree about what a link
        // meant.
        let mut d = testing::Origin::new(0);
        let first = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "budget planning for next quarter"),
            1_700_000_000_000,
        );
        let first_id = first.header.id();
        let slug = fold(core::slice::from_ref(&first)).notes[&first_id]
            .slug
            .clone();
        let second = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(2, &alloc::format!("see [[{slug}]]")),
            1_700_000_001_000,
        );
        let second_id = second.header.id();

        let forward = fold(&[first.clone(), second.clone()]);
        let backward = fold(&[second, first]);
        assert_eq!(
            forward.notes[&second_id].links,
            backward.notes[&second_id].links
        );
        assert_eq!(forward.notes[&second_id].links[0].resolved, Some(first_id));
    }

    #[test]
    fn a_link_written_before_its_target_existed_stays_unresolved() {
        // The rule that makes links trustworthy. The name matches a note that
        // exists *now*, and the link still resolves to nothing, because it
        // meant nothing when it was written.
        let mut d = testing::Origin::new(0);
        let early = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "see [[budget-planning-quarter-later]] soon"),
            1_700_000_000_000,
        );
        let early_id = early.header.id();
        let p = fold(&[early]);
        assert_eq!(p.notes[&early_id].links[0].resolved, None);
    }

    /// One note, with follow-up events authored against it.
    fn note_with(extra: impl Fn(&mut testing::Origin, EventId) -> Vec<Envelope>) -> Projection {
        let mut d = testing::Origin::new(0);
        let cap = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "budget planning for next quarter"),
            1_700_000_000_000,
        );
        let tgt = cap.header.id();
        let mut events = alloc::vec![cap];
        events.extend(extra(&mut d, tgt));
        fold(&events)
    }

    #[test]
    fn a_note_can_be_pinned_and_unpinned() {
        // The reason pin carries its value: an event that only ever sets a flag
        // needs a second kind to clear it, and two kinds for one boolean is a
        // conflict rule waiting to be got wrong.
        let p = note_with(|d, tgt| {
            alloc::vec![d.author(
                Kind::PIN,
                Some(tgt),
                None,
                testing::pin_body(2, true),
                1_700_000_001_000
            )]
        });
        assert!(p.notes.values().next().unwrap().pinned);

        let p = note_with(|d, tgt| {
            alloc::vec![
                d.author(
                    Kind::PIN,
                    Some(tgt),
                    None,
                    testing::pin_body(2, true),
                    1_700_000_001_000
                ),
                d.author(
                    Kind::PIN,
                    Some(tgt),
                    None,
                    testing::pin_body(3, false),
                    1_700_000_002_000
                ),
            ]
        });
        assert!(!p.notes.values().next().unwrap().pinned);
    }

    #[test]
    fn an_alias_names_a_note() {
        let p = note_with(|d, tgt| {
            alloc::vec![d.author(
                Kind::ALIAS,
                Some(tgt),
                None,
                testing::alias_body(2, Some("q4-budget")),
                1_700_000_001_000
            )]
        });
        assert_eq!(
            p.notes.values().next().unwrap().alias.as_deref(),
            Some("q4-budget")
        );
    }

    #[test]
    fn an_alias_can_be_cleared() {
        let p = note_with(|d, tgt| {
            alloc::vec![
                d.author(
                    Kind::ALIAS,
                    Some(tgt),
                    None,
                    testing::alias_body(2, Some("q4-budget")),
                    1_700_000_001_000
                ),
                d.author(
                    Kind::ALIAS,
                    Some(tgt),
                    None,
                    testing::alias_body(3, None),
                    1_700_000_002_000
                ),
            ]
        });
        assert_eq!(p.notes.values().next().unwrap().alias, None);
    }

    #[test]
    fn an_alias_cannot_shadow_another_notes_slug() {
        // One namespace. If an alias could take a slug, a written link would
        // mean different things depending on which namespace a reader consulted
        // first — and the link would be silently repointed, not broken.
        let mut d = testing::Origin::new(0);
        let first = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "budget planning for next quarter"),
            1_700_000_000_000,
        );
        let first_id = first.header.id();
        let victim_slug = fold(core::slice::from_ref(&first)).notes[&first_id]
            .slug
            .clone();

        let second = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(2, "something else entirely"),
            1_700_000_001_000,
        );
        let second_id = second.header.id();
        let grab = d.author(
            Kind::ALIAS,
            Some(second_id),
            None,
            testing::alias_body(3, Some(&victim_slug)),
            1_700_000_002_000,
        );

        let p = fold(&[first, second, grab]);
        assert_eq!(
            p.notes[&second_id].alias, None,
            "an alias that collides with a slug is declined"
        );
        assert_eq!(
            p.notes[&first_id].slug, victim_slug,
            "and the note that owns the name keeps it"
        );
    }

    #[test]
    fn two_notes_cannot_hold_the_same_alias() {
        let mut d = testing::Origin::new(0);
        let a = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "the first note here"),
            1_700_000_000_000,
        );
        let b = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(2, "the second note here"),
            1_700_000_001_000,
        );
        let (a_id, b_id) = (a.header.id(), b.header.id());
        let first = d.author(
            Kind::ALIAS,
            Some(a_id),
            None,
            testing::alias_body(3, Some("shared")),
            1_700_000_002_000,
        );
        let second = d.author(
            Kind::ALIAS,
            Some(b_id),
            None,
            testing::alias_body(4, Some("shared")),
            1_700_000_003_000,
        );

        let p = fold(&[a, b, first, second]);
        let holders = p
            .notes
            .values()
            .filter(|n| n.alias.as_deref() == Some("shared"))
            .count();
        assert_eq!(holders, 1, "a name denotes exactly one note");
    }

    #[test]
    fn a_dismissal_clears_only_the_versions_it_named() {
        // A divergence that turned up after the user looked must not be swept
        // away by a dismissal authored before it existed.
        let s = testing::branch_vs_single(1);
        let tgt = s.notes[0];
        let before = fold(&s.events);
        let divergences = before.notes[&tgt].divergences.clone();
        assert!(!divergences.is_empty(), "the fixture diverges");

        let mut d = testing::Origin::new(9);
        let dismissal = d.author(
            Kind::RESOLVE,
            Some(tgt),
            None,
            testing::resolve_body(1, alloc::vec![divergences[0].other]),
            1_700_000_100_000,
        );
        let mut events = s.events.clone();
        events.push(dismissal);

        let after = fold(&events);
        assert_eq!(
            after.notes[&tgt].divergences.len(),
            divergences.len() - 1,
            "exactly the named version is cleared"
        );
    }

    #[test]
    fn a_dismissal_naming_nothing_relevant_changes_nothing() {
        let s = testing::branch_vs_single(1);
        let tgt = s.notes[0];
        let before = fold(&s.events).notes[&tgt].divergences.len();

        let mut d = testing::Origin::new(9);
        let dismissal = d.author(
            Kind::RESOLVE,
            Some(tgt),
            None,
            testing::resolve_body(1, alloc::vec![EventId([0xAB; 32])]),
            1_700_000_100_000,
        );
        let mut events = s.events.clone();
        events.push(dismissal);
        assert_eq!(fold(&events).notes[&tgt].divergences.len(), before);
    }

    #[test]
    fn a_link_can_be_written_against_an_alias() {
        let mut d = testing::Origin::new(0);
        let target = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "budget planning for next quarter"),
            1_700_000_000_000,
        );
        let target_id = target.header.id();
        let named = d.author(
            Kind::ALIAS,
            Some(target_id),
            None,
            testing::alias_body(2, Some("q4-budget")),
            1_700_000_001_000,
        );
        let linker = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(3, "see [[q4-budget]] for figures"),
            1_700_000_002_000,
        );
        let linker_id = linker.header.id();

        let p = fold(&[target, named, linker]);
        assert_eq!(
            p.notes[&linker_id].links[0].resolved,
            Some(target_id),
            "an alias is a name like any other"
        );
    }

    #[test]
    fn a_capture_whose_body_is_gone_is_still_a_note() {
        // The content was destroyed — redacted, or stripped on the way into an
        // export. Losing the text is the intended cost; losing the note is data
        // loss, and it would happen silently on the next rebuild.
        let mut d = testing::Origin::new(0);
        let mut cap = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "something that was destroyed"),
            1_700_000_000_000,
        );
        let tgt = cap.header.id();
        cap.body = None;

        let p = fold(&[cap]);
        let n = p.notes.get(&tgt).expect("the note survives its content");
        assert_eq!(n.text, None);
        assert!(
            p.held.is_empty(),
            "an absent body is a known state, not a fault"
        );
    }

    #[test]
    fn a_bodyless_capture_is_not_named_after_what_it_said() {
        // The name is built from identity alone. Deriving it from the text
        // would leave the destroyed words in the one place that propagates into
        // every note linking here.
        let mut d = testing::Origin::new(0);
        let mut cap = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "quarterly swordfish migration meeting"),
            1_700_000_000_000,
        );
        let tgt = cap.header.id();
        let with_text = fold(core::slice::from_ref(&cap)).notes[&tgt].slug.clone();
        cap.body = None;
        let without = fold(&[cap]).notes[&tgt].slug.clone();

        assert!(with_text.contains("swordfish"), "got {with_text}");
        assert!(!without.contains("swordfish"), "got {without}");
        assert!(!without.contains("migration"), "got {without}");
    }

    #[test]
    fn a_body_that_is_present_but_unreadable_is_still_a_fault() {
        // The distinction the previous two tests rest on. A destroyed body is a
        // known state; a body this build cannot interpret is damage, and
        // treating them alike would hide real corruption.
        let mut d = testing::Origin::new(0);
        let mut cap = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "ordinary text"),
            1_700_000_000_000,
        );
        cap.body = Some(alloc::vec![0xFF, 0xFF, 0xFF]);
        let id = cap.header.id();

        let p = fold(&[cap]);
        assert_eq!(p.held.get(&id), Some(&Held::Malformed));
        assert!(p.notes.is_empty());
    }

    // -------------------------------------------------------- promotion ----

    fn promote_body(pinned: bool, grammar: Option<u32>, done: bool) -> alloc::vec::Vec<u8> {
        testing::encode(&crate::body::PromoteBody {
            nonce: [0u8; 16],
            as_: crate::body::Promoted::Task(crate::body::TaskFields {
                done,
                due: None,
                order: None,
            }),
            pinned,
            grammar,
            confidence: None,
            released: None,
        })
    }

    fn occ_body(occ: i64) -> alloc::vec::Vec<u8> {
        testing::encode(&crate::body::OccBody {
            nonce: [0u8; 16],
            occ,
        })
    }

    #[test]
    fn a_promotion_annotates_without_touching_the_text() {
        // The guarantee that makes a wrong interpretation survivable: a
        // misparse is a wrong label on an intact note, never a lost note.
        let s = testing::mixed(60, 1, 1, 0);
        let tgt = s.notes[0];
        let before = fold(&s.events).notes[&tgt].text.clone();

        let mut set = s.events.clone();
        set.push(testing::Origin::new(9).author(
            Kind::PROMOTE,
            Some(tgt),
            None,
            promote_body(false, Some(1), false),
            1_700_000_100_000,
        ));

        let n = &fold(&set).notes[&tgt];
        assert!(n.promotion.is_some(), "the promotion was not applied");
        assert_eq!(n.text, before, "promotion rewrote the captured text");
    }

    #[test]
    fn a_human_promotion_beats_a_later_machine_one() {
        // The one rule in this fold that is not plain last-write-wins. A
        // reparse carries a later HLC than a correction made by hand last
        // month, and under LWW it would silently revert it.
        let s = testing::mixed(61, 1, 1, 0);
        let tgt = s.notes[0];
        let mut set = s.events.clone();
        let mut d = testing::Origin::new(9);

        set.push(d.author(
            Kind::PROMOTE,
            Some(tgt),
            None,
            promote_body(true, None, true),
            1_700_000_100_000,
        ));
        // The parser, improved, sweeping the backlog a month later.
        set.push(d.author(
            Kind::PROMOTE,
            Some(tgt),
            None,
            promote_body(false, Some(2), false),
            1_800_000_000_000,
        ));

        let p = fold(&set).notes[&tgt].promotion.clone().unwrap();
        assert!(p.pinned, "a reparse overrode a human correction");
        assert!(
            matches!(
                p.as_,
                crate::body::Promoted::Task(crate::body::TaskFields { done: true, .. })
            ),
            "the human's fields were replaced"
        );
    }

    /// Which promotion won, by the `order` each test body carries as a label.
    fn label_of(set: &[Envelope], tgt: EventId) -> Option<u32> {
        match &fold(set).notes[&tgt].promotion.as_ref()?.as_ {
            crate::body::Promoted::Task(t) => t.order.map(|o| o as u32),
            _ => None,
        }
    }

    /// A promotion labelled `label`: a parser's, a pin, or a release.
    fn labelled(pinned: bool, released: bool, label: u32) -> alloc::vec::Vec<u8> {
        testing::encode(&crate::body::PromoteBody {
            nonce: [label as u8; 16],
            as_: crate::body::Promoted::Task(crate::body::TaskFields {
                done: false,
                due: None,
                order: Some(label as _),
            }),
            pinned,
            grammar: (!pinned).then_some(9),
            confidence: None,
            released: released.then_some(true),
        })
    }

    #[test]
    fn a_person_can_take_a_correction_back_in_any_arrival_order() {
        // Found in use on 2026-10-13: a suggestion accepted by mistake pinned
        // the wrong day and nothing could move it. A release is the person's
        // later word; after it, the parser competes by recency again.
        let s = testing::mixed(63, 1, 1, 0);
        let tgt = s.notes[0];
        let mut d = testing::Origin::new(9);
        let at = |n: u64| 1_700_000_100_000 + n * 1000;
        let parsed = d.author(
            Kind::PROMOTE,
            Some(tgt),
            None,
            labelled(false, false, 1),
            at(1),
        );
        let pin = d.author(
            Kind::PROMOTE,
            Some(tgt),
            None,
            labelled(true, false, 2),
            at(2),
        );
        let release = d.author(
            Kind::PROMOTE,
            Some(tgt),
            None,
            labelled(false, true, 3),
            at(3),
        );
        let edited = d.author(
            Kind::PROMOTE,
            Some(tgt),
            None,
            labelled(false, false, 4),
            at(4),
        );

        let orders: [[usize; 3]; 6] = [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        let three = [parsed.clone(), pin.clone(), release.clone()];
        for o in orders {
            let mut set = s.events.clone();
            set.extend(o.iter().map(|&i| three[i].clone()));
            assert_eq!(
                label_of(&set, tgt),
                Some(3),
                "the release stands, order {o:?}"
            );

            // A parse after the release — the next edit — is what shows.
            set.push(edited.clone());
            assert_eq!(label_of(&set, tgt), Some(4), "order {o:?}");
            let p = fold(&set).notes[&tgt].promotion.clone().unwrap();
            assert!(!p.pinned);
        }

        // And a correction made after the release is a pin again.
        let again = d.author(
            Kind::PROMOTE,
            Some(tgt),
            None,
            labelled(true, false, 5),
            at(5),
        );
        let mut set = s.events.clone();
        set.extend([again, edited, release, pin, parsed]);
        assert_eq!(label_of(&set, tgt), Some(5));
    }

    #[test]
    fn a_person_can_still_overrule_the_parser() {
        // The other direction has to work, or a wrong machine promotion would
        // be permanent.
        let s = testing::mixed(62, 1, 1, 0);
        let tgt = s.notes[0];
        let mut set = s.events.clone();
        let mut d = testing::Origin::new(9);

        set.push(d.author(
            Kind::PROMOTE,
            Some(tgt),
            None,
            promote_body(false, Some(1), false),
            1_700_000_100_000,
        ));
        set.push(d.author(
            Kind::PROMOTE,
            Some(tgt),
            None,
            promote_body(true, None, true),
            1_700_000_200_000,
        ));

        assert!(fold(&set).notes[&tgt].promotion.as_ref().unwrap().pinned);
    }

    #[test]
    fn two_machine_promotions_resolve_by_recency() {
        // Equal authority falls back to the ordinary rule.
        let s = testing::mixed(63, 1, 1, 0);
        let tgt = s.notes[0];
        let mut set = s.events.clone();
        let mut d = testing::Origin::new(9);

        set.push(d.author(
            Kind::PROMOTE,
            Some(tgt),
            None,
            promote_body(false, Some(1), false),
            1_700_000_100_000,
        ));
        set.push(d.author(
            Kind::PROMOTE,
            Some(tgt),
            None,
            promote_body(false, Some(2), true),
            1_700_000_200_000,
        ));

        let p = fold(&set).notes[&tgt].promotion.clone().unwrap();
        assert_eq!(p.grammar, Some(2), "the earlier parse won");
    }

    // ------------------------------------------------------- occurrences ----

    #[test]
    fn completing_an_occurrence_twice_is_completing_it_once() {
        let s = testing::mixed(64, 1, 1, 0);
        let tgt = s.notes[0];
        let mut set = s.events.clone();
        let mut d = testing::Origin::new(9);

        for t in [1_700_000_100_000, 1_700_000_200_000] {
            set.push(d.author(Kind::OCC_DONE, Some(tgt), None, occ_body(42), t));
        }

        let n = &fold(&set).notes[&tgt];
        assert_eq!(n.occurrences.len(), 1);
        assert!(n.occurrences[&42].done);
    }

    #[test]
    fn a_mistaken_tick_can_be_undone() {
        // Not a pure union, deliberately — un-completing has to be possible.
        let s = testing::mixed(65, 1, 1, 0);
        let tgt = s.notes[0];
        let mut set = s.events.clone();
        let mut d = testing::Origin::new(9);

        set.push(d.author(
            Kind::OCC_DONE,
            Some(tgt),
            None,
            occ_body(42),
            1_700_000_100_000,
        ));
        set.push(d.author(
            Kind::OCC_UNDONE,
            Some(tgt),
            None,
            occ_body(42),
            1_700_000_200_000,
        ));

        assert!(!fold(&set).notes[&tgt].occurrences[&42].done);
    }

    #[test]
    fn a_move_beats_a_cancel_and_the_cancel_is_kept() {
        // A resurrected occurrence is visible and one gesture to dismiss; a
        // wrongly cancelled one is invisible and silently missed (§6).
        let s = testing::mixed(66, 1, 1, 0);
        let tgt = s.notes[0];
        let mut d = testing::Origin::new(9);

        let cancel = d.author(
            Kind::OCC_CANCEL,
            Some(tgt),
            None,
            occ_body(42),
            1_700_000_100_000,
        );
        let moved = d.author(
            Kind::OCC_MOVE,
            Some(tgt),
            None,
            testing::encode(&crate::body::OccMoveBody {
                nonce: [0u8; 16],
                occ: 42,
                start_ms: 99,
                end_ms: None,
            }),
            1_700_000_200_000,
        );

        // Either arrival order, same answer.
        for order in [
            alloc::vec![cancel.clone(), moved.clone()],
            alloc::vec![moved.clone(), cancel.clone()],
        ] {
            let mut set = s.events.clone();
            set.extend(order);
            let o = &fold(&set).notes[&tgt].occurrences[&42];
            assert!(
                o.cancelled,
                "the cancellation was discarded rather than kept"
            );
            assert_eq!(o.moved_to, Some((99, None)));
            assert!(o.visible(), "a moved occurrence was hidden by a cancel");
        }
    }

    /// Skipping a moved occurrence hides it, and skip, restore, skip ends
    /// skipped.
    ///
    /// The bias above is right for a skip that came *first*: a later move is
    /// somebody wanting it to happen. But it made every skip after a move a
    /// silent no-op, and Restore is written as a move back to the original
    /// time, so a second skip after a restore never took either — found on the
    /// phone's calendar, where Skip and Restore are one tap apart. The one
    /// gesture that was meant to dismiss a resurrected occurrence did nothing.
    #[test]
    fn a_skip_after_a_move_hides_it() {
        let s = testing::mixed(69, 1, 1, 0);
        let tgt = s.notes[0];
        let mut d = testing::Origin::new(9);
        let skip = |d: &mut testing::Origin, at| {
            d.author(Kind::OCC_CANCEL, Some(tgt), None, occ_body(42), at)
        };
        let back = |d: &mut testing::Origin, at| {
            d.author(
                Kind::OCC_MOVE,
                Some(tgt),
                None,
                testing::encode(&crate::body::OccMoveBody {
                    nonce: [0u8; 16],
                    occ: 42,
                    start_ms: 42,
                    end_ms: None,
                }),
                at,
            )
        };

        let first = skip(&mut d, 1_700_000_100_000);
        let restored = back(&mut d, 1_700_000_200_000);
        let again = skip(&mut d, 1_700_000_300_000);

        for order in [
            alloc::vec![first.clone(), restored.clone(), again.clone()],
            alloc::vec![again.clone(), restored.clone(), first.clone()],
        ] {
            let mut set = s.events.clone();
            set.extend(order);
            let o = &fold(&set).notes[&tgt].occurrences[&42];
            assert!(!o.visible(), "the second skip did not take");
        }

        // And a restore after that brings it back again.
        let mut set = s.events.clone();
        set.extend([first, restored, again, back(&mut d, 1_700_000_400_000)]);
        assert!(fold(&set).notes[&tgt].occurrences[&42].visible());
    }

    #[test]
    fn a_cancelled_occurrence_is_hidden_but_not_gone() {
        let s = testing::mixed(67, 1, 1, 0);
        let tgt = s.notes[0];
        let mut set = s.events.clone();
        set.push(testing::Origin::new(9).author(
            Kind::OCC_CANCEL,
            Some(tgt),
            None,
            occ_body(42),
            1_700_000_100_000,
        ));

        let o = &fold(&set).notes[&tgt].occurrences[&42];
        assert!(!o.visible());
        assert!(o.cancelled, "the cancellation itself must remain recorded");
    }

    #[test]
    fn editing_one_detail_does_not_erase_another() {
        // Absent means unchanged, not cleared. Two devices editing different
        // details of one occurrence must not erase each other.
        let s = testing::mixed(68, 1, 1, 0);
        let tgt = s.notes[0];
        let mut set = s.events.clone();
        let mut d = testing::Origin::new(9);

        set.push(d.author(
            Kind::OCC_EDIT,
            Some(tgt),
            None,
            testing::encode(&crate::body::OccEditBody {
                nonce: [0u8; 16],
                occ: 42,
                title: Some(String::from("small room")),
                location: None,
                notes: None,
            }),
            1_700_000_100_000,
        ));
        set.push(d.author(
            Kind::OCC_EDIT,
            Some(tgt),
            None,
            testing::encode(&crate::body::OccEditBody {
                nonce: [1u8; 16],
                occ: 42,
                title: None,
                location: Some(String::from("upstairs")),
                notes: None,
            }),
            1_700_000_200_000,
        ));

        let o = &fold(&set).notes[&tgt].occurrences[&42];
        assert_eq!(o.title.as_deref(), Some("small room"), "a title was erased");
        assert_eq!(o.location.as_deref(), Some("upstairs"));
    }

    #[test]
    fn an_exception_for_an_unknown_occurrence_is_kept() {
        // The rule can be edited later, and a completion that vanished because
        // the schedule moved would be a completion silently lost.
        let s = testing::mixed(69, 1, 1, 0);
        let tgt = s.notes[0];
        let mut set = s.events.clone();
        set.push(testing::Origin::new(9).author(
            Kind::OCC_DONE,
            Some(tgt),
            None,
            occ_body(i64::MAX),
            1_700_000_100_000,
        ));

        assert!(fold(&set).notes[&tgt].occurrences.contains_key(&i64::MAX));
    }

    /// A whole-note redaction naming one target.
    fn redact_body(tgt: EventId) -> alloc::vec::Vec<u8> {
        testing::encode(&crate::body::RedactBody {
            nonce: [0u8; 16],
            scope: crate::body::RedactScope::WholeNote,
            targets: alloc::vec![tgt],
        })
    }

    fn mute_body(
        nonce: u8,
        scope: crate::body::MuteScope,
        until: Option<u64>,
    ) -> alloc::vec::Vec<u8> {
        testing::encode(&crate::body::MuteBody {
            nonce: [nonce; 16],
            scope,
            until,
        })
    }

    #[test]
    fn a_note_can_be_muted_by_name() {
        let p = note_with(|d, tgt| {
            alloc::vec![d.author(
                Kind::MUTE,
                Some(tgt),
                None,
                mute_body(2, crate::body::MuteScope::Note(tgt), None),
                1_700_000_001_000
            )]
        });
        assert_eq!(p.notes.values().next().unwrap().muted_until, Some(0));
    }

    #[test]
    fn muting_a_tag_covers_every_note_carrying_it() {
        // A mute is a rule, not a property of one note. Applying it to whatever
        // happened to exist when it was written would miss everything that
        // qualified afterwards.
        let mut d = testing::Origin::new(0);
        let a = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "the first note here"),
            1_700_000_000_000,
        );
        let b = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(2, "the second note here"),
            1_700_000_001_000,
        );
        let (a_id, b_id) = (a.header.id(), b.header.id());

        let tag_a = d.author(
            Kind::TAG,
            Some(a_id),
            None,
            testing::tag_body(3, "noise"),
            1_700_000_002_000,
        );
        // The mute is written before the second note is tagged.
        let mute = d.author(
            Kind::MUTE,
            None,
            None,
            mute_body(4, crate::body::MuteScope::Tag("noise".into()), None),
            1_700_000_003_000,
        );
        let tag_b = d.author(
            Kind::TAG,
            Some(b_id),
            None,
            testing::tag_body(5, "noise"),
            1_700_000_004_000,
        );

        let p = fold(&[a, b, tag_a, mute, tag_b]);
        assert_eq!(p.notes[&a_id].muted_until, Some(0));
        assert_eq!(
            p.notes[&b_id].muted_until,
            Some(0),
            "a note tagged after the mute was written is covered too"
        );
    }

    #[test]
    fn untagging_lifts_a_tag_mute() {
        // The rule follows the tag. A note that no longer carries it is no
        // longer covered, without anybody having to remember to unmute it.
        let mut d = testing::Origin::new(0);
        let cap = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "the only note here"),
            1_700_000_000_000,
        );
        let tgt = cap.header.id();
        let tag = d.author(
            Kind::TAG,
            Some(tgt),
            None,
            testing::tag_body(2, "noise"),
            1_700_000_001_000,
        );
        let tag_id = tag.header.id();
        let mute = d.author(
            Kind::MUTE,
            None,
            None,
            mute_body(3, crate::body::MuteScope::Tag("noise".into()), None),
            1_700_000_002_000,
        );
        let untag = d.author(
            Kind::UNTAG,
            Some(tgt),
            None,
            testing::untag_body(4, alloc::vec![tag_id]),
            1_700_000_003_000,
        );

        let p = fold(&[cap, tag, mute, untag]);
        assert_eq!(p.notes[&tgt].muted_until, None);
    }

    #[test]
    fn a_date_range_covers_the_days_inside_it() {
        // 1_700_000_000_000 is 2023-11-14 in UTC.
        let p = note_with(|d, _tgt| {
            alloc::vec![d.author(
                Kind::MUTE,
                None,
                None,
                mute_body(
                    2,
                    crate::body::MuteScope::DateRange("2023-11-01".into(), "2023-11-30".into()),
                    None
                ),
                1_700_000_001_000
            )]
        });
        assert_eq!(p.notes.values().next().unwrap().muted_until, Some(0));
    }

    #[test]
    fn a_date_range_leaves_days_outside_it_alone() {
        let p = note_with(|d, _tgt| {
            alloc::vec![d.author(
                Kind::MUTE,
                None,
                None,
                mute_body(
                    2,
                    crate::body::MuteScope::DateRange("2024-01-01".into(), "2024-01-31".into()),
                    None
                ),
                1_700_000_001_000
            )]
        });
        assert_eq!(p.notes.values().next().unwrap().muted_until, None);
    }

    #[test]
    fn a_mute_can_expire() {
        // An expiry is stored, not enforced here: whether the moment has passed
        // is a question about now, and the fold cannot ask it.
        let until = 1_800_000_000_000;
        let p = note_with(|d, tgt| {
            alloc::vec![d.author(
                Kind::MUTE,
                Some(tgt),
                None,
                mute_body(2, crate::body::MuteScope::Note(tgt), Some(until)),
                1_700_000_001_000
            )]
        });
        assert_eq!(p.notes.values().next().unwrap().muted_until, Some(until));
    }

    #[test]
    fn the_newest_mute_touching_a_note_wins() {
        let mut d = testing::Origin::new(0);
        let cap = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "the only note here"),
            1_700_000_000_000,
        );
        let tgt = cap.header.id();
        let forever = d.author(
            Kind::MUTE,
            Some(tgt),
            None,
            mute_body(2, crate::body::MuteScope::Note(tgt), None),
            1_700_000_001_000,
        );
        let until = d.author(
            Kind::MUTE,
            Some(tgt),
            None,
            mute_body(
                3,
                crate::body::MuteScope::Note(tgt),
                Some(1_800_000_000_000),
            ),
            1_700_000_002_000,
        );

        let p = fold(&[cap, forever, until]);
        assert_eq!(p.notes[&tgt].muted_until, Some(1_800_000_000_000));
    }

    #[test]
    fn days_convert_correctly() {
        // The arithmetic that replaces a date library. Wrong by one and every
        // range mute is off by a day.
        assert_eq!(civil_from_days(0), "1970-01-01");
        assert_eq!(civil_from_days(19_675), "2023-11-14");
        assert_eq!(civil_from_days(-1), "1969-12-31");
    }

    #[test]
    fn shuffling_the_input_cannot_change_the_output() {
        // The flagship property. If this fails, two devices holding identical
        // events can show different things.
        let s = testing::mixed(1, 3, 5, 40);
        let base = fold(&s.events).canonical();
        for seed in [7u64, 99, 12345, 2, 808] {
            assert_eq!(
                fold(&s.shuffled(seed)).canonical(),
                base,
                "fold must not depend on arrival order (seed {seed})"
            );
        }
    }

    #[test]
    fn shuffling_cannot_change_the_output_for_the_typed_kinds_either() {
        // The flagship property is only worth what the generator produces, and
        // `mixed` emits captures, edits and tags — none of the kinds M2 added.
        // Promotions resolve by authority before recency, which is the one rule
        // here that is not plain last-write-wins and therefore the one most
        // likely to be order-dependent by accident.
        for seed in [3u64, 11, 404] {
            let s = testing::typed(seed);
            let base = fold(&s.events).canonical();
            for shuffle in [1u64, 42, 9001] {
                assert_eq!(
                    fold(&s.shuffled(shuffle)).canonical(),
                    base,
                    "the typed kinds depend on arrival order (seed {seed}, shuffle {shuffle})"
                );
            }
        }
    }

    #[test]
    fn a_pinned_promotion_survives_every_arrival_order() {
        // Stated on its own because a shuffle that happened to deliver the
        // human promotion last would pass the property above for the wrong
        // reason.
        for shuffle in [0u64, 5, 77, 1234, 65535] {
            let s = testing::typed(9);
            let p = fold(&s.shuffled(shuffle));
            for note in &s.notes {
                let promotion = p.notes[note]
                    .promotion
                    .as_ref()
                    .expect("every note in this scenario is promoted");
                assert!(
                    promotion.pinned,
                    "a machine promotion displaced a human one (shuffle {shuffle})"
                );
            }
        }
    }

    #[test]
    fn folding_is_stable_across_many_shapes() {
        for seed in 1..12u64 {
            let s = testing::mixed(seed, 2 + (seed as usize % 3), 3, 20);
            assert_eq!(
                fold(&s.events).canonical(),
                fold(&s.shuffled(seed * 7)).canonical()
            );
        }
    }

    #[test]
    fn a_capture_becomes_a_note() {
        let s = testing::mixed(2, 1, 1, 0);
        let p = fold(&s.events);
        assert_eq!(p.notes.len(), 1);
        let n = p.notes.values().next().unwrap();
        assert!(n.text.is_some());
        assert_eq!(n.head, n.versions[0].id);
    }

    #[test]
    fn sequential_edits_do_not_diverge() {
        let s = testing::mixed(4, 1, 1, 10);
        let p = fold(&s.events);
        for n in p.notes.values() {
            assert!(
                n.divergences.is_empty(),
                "edits made in sequence must not be reported as independent"
            );
        }
    }

    #[test]
    fn independent_edits_diverge_exactly_once() {
        let s = testing::concurrent_edit(1);
        let p = fold(&s.events);
        let n = p.notes.get(&s.notes[0]).unwrap();
        assert_eq!(n.divergences.len(), 1);
    }

    #[test]
    fn both_independent_versions_are_kept() {
        // Losing an edit is worse than showing the wrong one.
        let s = testing::concurrent_edit(1);
        let p = fold(&s.events);
        let n = p.notes.get(&s.notes[0]).unwrap();
        assert_eq!(n.versions.len(), 3, "capture plus both edits");
        let d = &n.divergences[0];
        assert!(n.versions.iter().any(|v| v.id == d.current));
        assert!(n.versions.iter().any(|v| v.id == d.other));
    }

    #[test]
    fn a_branch_against_a_single_edit_diverges_once() {
        // The case a single-hop comparison misses entirely: neither head names
        // the other, and they share no predecessor.
        let s = testing::branch_vs_single(1);
        let p = fold(&s.events);
        let n = p.notes.get(&s.notes[0]).unwrap();
        assert_eq!(
            n.divergences.len(),
            1,
            "a two-edit branch against a one-edit branch is one divergence"
        );
        assert_eq!(n.versions.len(), 4, "capture plus three edits, all kept");
    }

    #[test]
    fn a_concurrent_add_survives_a_remove_that_never_saw_it() {
        let s = testing::concurrent_tag(1);
        let p = fold(&s.events);
        let n = p.notes.get(&s.notes[0]).unwrap();
        assert!(
            n.tags.contains("urgent"),
            "a removal may only retract additions it observed"
        );
    }

    #[test]
    fn tags_converge_regardless_of_order() {
        let s = testing::concurrent_tag(1);
        let a = fold(&s.events);
        let b = fold(&s.shuffled(31));
        assert_eq!(a.canonical(), b.canonical());
    }

    #[test]
    fn an_unknown_kind_is_held_rather_than_folded() {
        let s = testing::unknown_kind(3);
        let p = fold(&s.events);
        assert!(
            p.held.values().any(|h| *h == Held::UnknownKind),
            "a build that cannot interpret an event must still record it"
        );
    }

    #[test]
    fn holding_an_event_does_not_change_the_rest() {
        let plain = testing::mixed(3, 2, 2, 4);
        let with_unknown = testing::unknown_kind(3);
        let a = fold(&plain.events);
        let b = fold(&with_unknown.events);
        assert_eq!(
            a.notes.len(),
            b.notes.len(),
            "an uninterpretable event must not disturb the notes around it"
        );
    }

    #[test]
    fn a_superseded_divergence_stops_being_one() {
        // The reason divergences are computed from the finished graph rather
        // than as events arrive. Mid-stream, A's first edit and B's edit are
        // genuinely independent — but A's second edit supersedes A's first, so
        // by the end only two branch tips exist, not three.
        let s = testing::branch_vs_single(1);
        let p = fold(&s.events);
        let n = p.notes.get(&s.notes[0]).unwrap();
        assert_eq!(n.divergences.len(), 1);
        // And the surviving pair is between the two tips.
        let d = &n.divergences[0];
        let superseded: alloc::vec::Vec<_> = n.versions.iter().filter_map(|v| v.prev).collect();
        assert!(!superseded.contains(&d.current));
        assert!(!superseded.contains(&d.other));
    }

    #[test]
    fn the_shown_version_is_a_branch_tip() {
        let s = testing::branch_vs_single(1);
        let p = fold(&s.events);
        let n = p.notes.get(&s.notes[0]).unwrap();
        let superseded: alloc::vec::Vec<_> = n.versions.iter().filter_map(|v| v.prev).collect();
        assert!(
            !superseded.contains(&n.head),
            "never show a version something else replaced"
        );
    }

    #[test]
    fn a_redaction_reports_destruction_whatever_order_it_arrives_in() {
        // The two-step model: the redaction event *orders* destruction, and the
        // purge carries it out by removing the body. The fold reports what the
        // log holds, so a redaction whose purge has run reads as destroyed from
        // either arrival order.
        let s = testing::mixed(8, 1, 1, 2);
        let tgt = s.notes[0];

        let mut d = testing::Origin::new(5);
        let r = d.author(
            Kind::REDACT,
            Some(tgt),
            None,
            redact_body(tgt),
            1_700_000_000_000,
        );

        // What the purge leaves behind: the ordering event, and bodies gone.
        let purged: Vec<Envelope> = s
            .events
            .iter()
            .cloned()
            .map(|mut e| {
                if e.header.tgt == Some(tgt) || e.header.id() == tgt {
                    e.body = None;
                }
                e
            })
            .collect();

        let mut after = purged.clone();
        after.push(r.clone());
        let mut before = alloc::vec![r];
        before.extend(purged);

        for set in [after, before] {
            let p = fold(&set);
            let n = p.notes.get(&tgt).unwrap();
            assert!(
                n.text.is_none(),
                "a redaction must not depend on arrival order"
            );
            assert!(n.is_redacted());
        }
    }

    #[test]
    fn a_redaction_alone_destroys_nothing_yet() {
        // The cancellation window, and the reason it can exist at all. The
        // ordering event does not remove anything; if it did, cancelling would
        // have nothing left to save.
        let s = testing::mixed(21, 1, 1, 1);
        let tgt = s.notes[0];
        let mut d = testing::Origin::new(5);

        let mut set = s.events.clone();
        set.push(d.author(
            Kind::REDACT,
            Some(tgt),
            None,
            redact_body(tgt),
            1_700_000_500_000,
        ));

        let p = fold(&set);
        let n = p.notes.get(&tgt).unwrap();
        assert!(n.text.is_some(), "the content is still readable");
        assert!(!n.is_redacted(), "and not reported as destroyed");
        assert!(n.redaction_pending.is_some(), "but a redaction is queued");
    }

    #[test]
    fn a_redaction_after_a_cancellation_still_takes_effect() {
        // The bug this exists for: the fold kept only the *first* redaction
        // naming a note. Cancel that one and the dead order was the only one on
        // record, so every later redaction of the same note was silently
        // discarded — content the user had ordered destroyed stayed readable,
        // was never queued, and nothing anywhere reported it.
        let s = testing::mixed(30, 1, 1, 0);
        let tgt = s.notes[0];
        let mut set = s.events.clone();

        let first = testing::Origin::new(9).author(
            Kind::REDACT,
            Some(tgt),
            None,
            redact_body(tgt),
            1_700_000_100_000,
        );
        let first_id = first.header.id();
        set.push(first);

        set.push(testing::Origin::new(9).author(
            Kind::REDACT_CANCEL,
            Some(tgt),
            None,
            testing::encode(&crate::body::RedactCancelBody {
                nonce: [1u8; 16],
                redact: first_id,
            }),
            1_700_000_200_000,
        ));

        // Cancelled: readable again, and nothing queued.
        let p = fold(&set);
        assert!(p.notes[&tgt].redaction_pending.is_none());

        // Ordered again. This has to stand on its own.
        set.push(testing::Origin::new(9).author(
            Kind::REDACT,
            Some(tgt),
            None,
            redact_body(tgt),
            1_700_000_300_000,
        ));

        let p = fold(&set);
        let n = &p.notes[&tgt];
        assert!(
            n.redaction_pending.is_some(),
            "a redaction issued after a cancellation was discarded"
        );
        assert_ne!(
            n.redaction_pending.unwrap().redact,
            first_id,
            "the cancelled redaction is governing a note it no longer governs"
        );
    }

    #[test]
    fn the_earliest_standing_redaction_governs() {
        // Two live redactions on one note. The earlier deadline wins, for the
        // same reason the window is measured from the redaction's own timestamp:
        // a later order must not grant the content a fresh window and so let it
        // outlive the first order.
        let s = testing::mixed(31, 1, 1, 0);
        let tgt = s.notes[0];
        let mut set = s.events.clone();

        let early = testing::Origin::new(9).author(
            Kind::REDACT,
            Some(tgt),
            None,
            redact_body(tgt),
            1_700_000_100_000,
        );
        let early_id = early.header.id();
        set.push(early);
        set.push(testing::Origin::new(9).author(
            Kind::REDACT,
            Some(tgt),
            None,
            redact_body(tgt),
            1_700_000_900_000,
        ));

        let p = fold(&set);
        assert_eq!(p.notes[&tgt].redaction_pending.unwrap().redact, early_id);
    }

    #[test]
    fn a_pending_redaction_is_due_from_its_own_timestamp() {
        // Never from local receipt. A per-replica window would mean a device
        // that was switched off for a month granting the content another full
        // window when it came back — the longer a replica is away, the longer
        // the material it was told to destroy survives.
        let s = testing::mixed(22, 1, 1, 0);
        let tgt = s.notes[0];
        let mut d = testing::Origin::new(5);
        let issued_at = 1_700_000_500_000;

        let mut set = s.events.clone();
        let r = d.author(Kind::REDACT, Some(tgt), None, redact_body(tgt), issued_at);
        let stamp = r.header.hlc.wall;
        set.push(r);

        let p = fold(&set);
        let pending = p.notes[&tgt].redaction_pending.unwrap();
        assert_eq!(pending.execute_after, stamp + crate::REDACT_WINDOW_MS);
    }

    #[test]
    fn a_cancellation_takes_the_redaction_out_of_the_queue() {
        let s = testing::mixed(23, 1, 1, 0);
        let tgt = s.notes[0];
        let mut d = testing::Origin::new(5);

        let r = d.author(
            Kind::REDACT,
            Some(tgt),
            None,
            redact_body(tgt),
            1_700_000_500_000,
        );
        let redact_id = r.header.id();
        let cancel = d.author(
            Kind::REDACT_CANCEL,
            Some(tgt),
            None,
            testing::encode(&crate::body::RedactCancelBody {
                nonce: [1u8; 16],
                redact: redact_id,
            }),
            1_700_000_600_000,
        );

        let mut set = s.events.clone();
        set.push(r);
        set.push(cancel);

        let p = fold(&set);
        let n = p.notes.get(&tgt).unwrap();
        assert!(n.redaction_pending.is_none(), "no longer queued");
        assert!(n.text.is_some(), "and the content was never touched");
    }

    #[test]
    fn a_cancellation_that_arrives_after_destruction_changes_nothing() {
        // Destruction is the one irreversible act in the system. A cancellation
        // that lost the race is reported, not honoured — there is nothing left
        // to honour it with.
        let s = testing::mixed(24, 1, 1, 0);
        let tgt = s.notes[0];
        let mut d = testing::Origin::new(5);

        let r = d.author(
            Kind::REDACT,
            Some(tgt),
            None,
            redact_body(tgt),
            1_700_000_500_000,
        );
        let redact_id = r.header.id();
        let cancel = d.author(
            Kind::REDACT_CANCEL,
            Some(tgt),
            None,
            testing::encode(&crate::body::RedactCancelBody {
                nonce: [1u8; 16],
                redact: redact_id,
            }),
            1_700_000_600_000,
        );

        // The purge already ran.
        let mut set: Vec<Envelope> = s
            .events
            .iter()
            .cloned()
            .map(|mut e| {
                if e.header.tgt == Some(tgt) || e.header.id() == tgt {
                    e.body = None;
                }
                e
            })
            .collect();
        set.push(r);
        set.push(cancel);

        let n = fold(&set).notes.get(&tgt).cloned().unwrap();
        assert!(n.is_redacted(), "the content is gone and stays gone");
        assert_eq!(n.text, None);
    }

    #[test]
    fn a_redacted_note_keeps_a_reconstructible_name() {
        use crate::body::{RedactBody, RedactScope};
        let s = testing::mixed(9, 1, 1, 0);
        let tgt = s.notes[0];
        let mut d = testing::Origin::new(6);
        let body = testing::encode(&RedactBody {
            nonce: [0u8; 16],
            scope: RedactScope::WholeNote,
            targets: alloc::vec![tgt],
        });
        let mut set: Vec<Envelope> = s
            .events
            .iter()
            .cloned()
            .map(|mut e| {
                if e.header.tgt == Some(tgt) || e.header.id() == tgt {
                    e.body = None;
                }
                e
            })
            .collect();
        set.push(d.author(Kind::REDACT, Some(tgt), None, body, 1_700_000_000_000));

        let n = fold(&set).notes.get(&tgt).cloned().unwrap();
        // Derived from identity alone: the row survives a rebuild although its
        // text is gone, and the name stops carrying the words it was built from.
        assert!(n.slug.starts_with("redacted-"));
        assert_eq!(n.slug, fold(&set).notes.get(&tgt).unwrap().slug);
    }

    #[test]
    fn a_deletion_loses_to_an_edit_made_independently() {
        let s = testing::mixed(10, 1, 1, 4);
        let tgt = s.notes[0];
        let mut d = testing::Origin::new(7);
        let body = testing::encode(&crate::body::TagBody {
            nonce: [0u8; 16],
            tag: alloc::string::String::from("ignored"),
        });
        let mut set = s.events.clone();
        set.push(d.author(Kind::TOMBSTONE, Some(tgt), None, body, 1_700_000_900_000));

        let p = fold(&set);
        let n = p.notes.get(&tgt).unwrap();
        assert!(
            n.text.is_some(),
            "an edit beats a deletion: a surviving note is visible, a lost edit is not"
        );
        assert!(
            n.versions
                .iter()
                .any(|v| v.kind == Kind::TOMBSTONE && v.excluded),
            "the deletion is still recorded"
        );
    }

    #[test]
    fn an_edit_for_a_note_we_do_not_have_is_held() {
        let s = testing::mixed(11, 1, 1, 3);
        // Drop the capture; the edits now refer to nothing.
        let orphans: alloc::vec::Vec<_> = s
            .events
            .iter()
            .filter(|e| e.header.kind != Kind::CAPTURE)
            .cloned()
            .collect();
        let p = fold(&orphans);
        assert!(p.notes.is_empty());
        assert!(
            p.held.values().any(|h| *h == Held::NoTarget),
            "an event whose note is absent is recorded, not discarded"
        );
    }

    #[test]
    fn distinct_tips_are_always_mutually_unreachable() {
        // Checks the equivalence the tip rule relies on: it replaced an
        // explicit reachability walk, and would be wrong if two live tips could
        // reach each other.
        for seed in 1..8u64 {
            let s = testing::mixed(seed, 3, 3, 25);
            let p = fold(&s.events);
            for n in p.notes.values() {
                let superseded: alloc::vec::Vec<_> = n
                    .versions
                    .iter()
                    .filter(|v| !v.excluded)
                    .filter_map(|v| v.prev)
                    .collect();
                let tips: alloc::vec::Vec<_> = n
                    .versions
                    .iter()
                    .filter(|v| !v.excluded && !superseded.contains(&v.id))
                    .collect();
                for a in &tips {
                    for b in &tips {
                        if a.id != b.id {
                            assert!(!reaches(&n.versions, a.id, b.id));
                            assert!(!reaches(&n.versions, b.id, a.id));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_sequential_edit_reaches_the_version_it_replaced() {
        let s = testing::branch_vs_single(1);
        let p = fold(&s.events);
        let n = p.notes.get(&s.notes[0]).unwrap();
        // A's second edit follows A's first, so it reaches it.
        let a2 = n.versions.iter().find(|v| {
            v.prev.is_some_and(|pr| {
                n.versions
                    .iter()
                    .any(|w| w.id == pr && w.prev == Some(n.versions[0].id))
            })
        });
        if let Some(a2) = a2 {
            assert!(reaches(&n.versions, a2.id, n.versions[0].id));
        }
    }

    #[test]
    fn the_projection_is_empty_for_no_events() {
        assert_eq!(fold(&[]).canonical(), Projection::default().canonical());
    }

    #[test]
    fn a_note_keeps_its_capture_timezone() {
        // Cannot be recovered later, so it must come from the capture itself.
        let s = testing::mixed(5, 1, 1, 0);
        let p = fold(&s.events);
        assert_eq!(
            p.notes.values().next().unwrap().tz.as_deref(),
            Some("Europe/London")
        );
    }

    #[test]
    fn folding_twice_gives_the_same_answer() {
        let s = testing::mixed(6, 2, 3, 12);
        assert_eq!(fold(&s.events).canonical(), fold(&s.events).canonical());
    }

    // ------------------------------------------------------------- parents ---
    //
    // The one hand-made structure here. §3.7's argument against filing stands;
    // what these pin is that a wrong or conflicting parent costs a branch of
    // one sidebar and nothing else.

    fn parented(
        events: &mut Vec<Envelope>,
        d: &mut testing::Origin,
        tgt: EventId,
        under: Option<EventId>,
        now: u64,
    ) {
        let body = testing::encode(&crate::body::ParentBody {
            nonce: [0u8; 16],
            parent: under,
        });
        events.push(d.author(Kind::PARENT, Some(tgt), None, body, now));
    }

    #[test]
    fn a_page_can_be_put_under_another_and_taken_back_out() {
        let s = testing::mixed(3, 1, 2, 0);
        let (a, b) = (s.notes[0], s.notes[1]);
        let mut d = testing::Origin::new(4);
        let mut set = s.events.clone();

        parented(&mut set, &mut d, a, Some(b), 1_700_000_100_000);
        assert_eq!(fold(&set).notes[&a].parent, Some(b));

        // `None` is a decision, not an absence: it has to beat the earlier
        // parent by ordinary last-write-wins, or nothing could ever be taken
        // back out of a branch it was filed into.
        parented(&mut set, &mut d, a, None, 1_700_000_200_000);
        assert_eq!(
            fold(&set).notes[&a].parent,
            None,
            "it could not be moved back to the top"
        );
    }

    #[test]
    fn a_note_cannot_be_its_own_parent() {
        // The one loop somebody can make on purpose, and it has no reading at
        // all — so it is refused where it is written rather than left to the
        // cycle pass.
        let s = testing::mixed(3, 1, 1, 0);
        let a = s.notes[0];
        let mut d = testing::Origin::new(4);
        let mut set = s.events.clone();

        parented(&mut set, &mut d, a, Some(a), 1_700_000_100_000);
        assert_eq!(fold(&set).notes[&a].parent, None);
    }

    #[test]
    fn a_loop_made_by_two_devices_is_broken_the_same_way_everywhere() {
        // Neither device is wrong when it writes: "A under B" is a fine thing
        // to say, and so is "B under A". Only the pair is a problem, and it
        // only exists once both are folded — which is where it can be resolved
        // identically on every replica.
        let s = testing::mixed(3, 2, 2, 0);
        let (a, b) = (s.notes[0], s.notes[1]);
        let mut one = testing::Origin::new(7);
        let mut two = testing::Origin::new(8);
        let mut set = s.events.clone();

        parented(&mut set, &mut one, a, Some(b), 1_700_000_100_000);
        parented(&mut set, &mut two, b, Some(a), 1_700_000_200_000);

        let folded = fold(&set);
        let broken = folded.notes[&a].parent.is_none() || folded.notes[&b].parent.is_none();
        assert!(broken, "a loop survived the fold");

        // And the same answer whatever order they arrive in, which is the
        // property the whole log rests on.
        let mut shuffled = set.clone();
        shuffled.reverse();
        let again = fold(&shuffled);
        assert_eq!(folded.notes[&a].parent, again.notes[&a].parent);
        assert_eq!(folded.notes[&b].parent, again.notes[&b].parent);
    }

    #[test]
    fn breaking_a_loop_keeps_the_older_arrangement() {
        // The newest edge is the one dropped. Same instinct as the rest of the
        // conflict rules: the surprising outcome should be the recent one,
        // because it is the one somebody still remembers making.
        let s = testing::mixed(3, 2, 2, 0);
        let (a, b) = (s.notes[0], s.notes[1]);
        let mut one = testing::Origin::new(7);
        let mut two = testing::Origin::new(8);
        let mut set = s.events.clone();

        parented(&mut set, &mut one, a, Some(b), 1_700_000_100_000);
        parented(&mut set, &mut two, b, Some(a), 1_700_000_900_000);

        let folded = fold(&set);
        assert_eq!(
            folded.notes[&a].parent,
            Some(b),
            "the older edge was dropped"
        );
        assert_eq!(
            folded.notes[&b].parent, None,
            "the newer edge survived a loop"
        );
    }

    #[test]
    fn a_longer_loop_is_broken_too() {
        let s = testing::mixed(3, 2, 3, 0);
        let (a, b, c) = (s.notes[0], s.notes[1], s.notes[2]);
        let mut d = testing::Origin::new(7);
        let mut set = s.events.clone();

        parented(&mut set, &mut d, a, Some(b), 1_700_000_100_000);
        parented(&mut set, &mut d, b, Some(c), 1_700_000_200_000);
        parented(&mut set, &mut d, c, Some(a), 1_700_000_300_000);

        let folded = fold(&set);
        let edges = [a, b, c]
            .iter()
            .filter(|n| folded.notes[n].parent.is_some())
            .count();
        assert_eq!(edges, 2, "a three-note loop was not opened");
    }

    #[test]
    fn a_parent_that_is_not_here_yet_is_not_a_loop() {
        // A forward reference, or a note this replica does not hold. The tree
        // draws the page at the top until the parent arrives; treating it as a
        // loop would drop an edge that is perfectly good.
        let s = testing::mixed(3, 1, 1, 0);
        let a = s.notes[0];
        let mut d = testing::Origin::new(4);
        let mut set = s.events.clone();

        let absent = EventId([0xAB; 32]);
        parented(&mut set, &mut d, a, Some(absent), 1_700_000_100_000);
        assert_eq!(fold(&set).notes[&a].parent, Some(absent));
    }
}

#[cfg(test)]
mod entity_tests {
    use super::*;
    use crate::body::{EntityBindBody, EntityMergeBody, EntityRenameBody};
    use crate::testing;

    fn enc<T: minicbor::Encode<()>>(v: &T) -> Vec<u8> {
        let mut b = Vec::new();
        minicbor::encode(v, &mut b).unwrap();
        b
    }

    fn sarah() -> EntId {
        EntId::derive("person", "sarah")
    }
    fn chen() -> EntId {
        EntId::derive("person", "sarah-chen")
    }

    #[test]
    fn identity_does_not_depend_on_which_spelling_or_what_order() {
        // The property the whole design rests on, and it holds by construction
        // rather than by a rule applied carefully: the hashed input is the
        // case-folded spelling, so nothing about when a name was seen can reach
        // the id.
        assert_eq!(EntId::derive("person", "SARAH"), sarah());
        assert_eq!(EntId::derive("person", "Sarah"), sarah());
        assert_ne!(
            sarah(),
            chen(),
            "a different spelling is a different entity"
        );
    }

    #[test]
    fn a_rename_changes_the_name_and_nothing_else() {
        let mut d = testing::Origin::new(0);
        let e = d.author(
            Kind::ENTITY_RENAME,
            None,
            None,
            enc(&EntityRenameBody {
                nonce: [1; 16],
                ent: sarah(),
                display: "Sarah Chen".into(),
            }),
            1_700_000_000_000,
        );
        let p = fold(&[e]);
        assert_eq!(p.entity_name(sarah()), Some("Sarah Chen"));
        // Identity untouched: the entity is still the one the spelling derives.
        assert_eq!(p.entity_of(sarah()), sarah());
    }

    #[test]
    fn a_later_rename_wins_and_an_earlier_one_does_not() {
        let mut d = testing::Origin::new(0);
        let first = d.author(
            Kind::ENTITY_RENAME,
            None,
            None,
            enc(&EntityRenameBody {
                nonce: [1; 16],
                ent: sarah(),
                display: "Sarah".into(),
            }),
            1_700_000_000_000,
        );
        let second = d.author(
            Kind::ENTITY_RENAME,
            None,
            None,
            enc(&EntityRenameBody {
                nonce: [2; 16],
                ent: sarah(),
                display: "Sarah Chen".into(),
            }),
            1_700_000_100_000,
        );
        // Both orders of delivery, because a phone reconnecting hands over the
        // older one second as a matter of course.
        assert_eq!(
            fold(&[first.clone(), second.clone()]).entity_name(sarah()),
            Some("Sarah Chen")
        );
        assert_eq!(
            fold(&[second, first]).entity_name(sarah()),
            Some("Sarah Chen"),
            "the older rename won when it arrived second"
        );
    }

    #[test]
    fn an_empty_name_is_refused_rather_than_shown() {
        let mut d = testing::Origin::new(0);
        let e = d.author(
            Kind::ENTITY_RENAME,
            None,
            None,
            enc(&EntityRenameBody {
                nonce: [1; 16],
                ent: sarah(),
                display: "   ".into(),
            }),
            1_700_000_000_000,
        );
        let id = e.header.id();
        let p = fold(&[e]);
        assert_eq!(p.entity_name(sarah()), None);
        assert_eq!(p.held.get(&id), Some(&Held::Malformed));
    }

    #[test]
    fn a_merge_chain_resolves_to_its_end() {
        // A into B into C. Both edges stay, and resolution walks them — which is
        // why this is stored as a chain rather than applied on the way in.
        let a = EntId::derive("person", "s");
        let b = EntId::derive("person", "sarah");
        let c = EntId::derive("person", "sarah-chen");

        let mut d = testing::Origin::new(0);
        let mut events = Vec::new();
        for (i, (from, into)) in [(a, b), (b, c)].into_iter().enumerate() {
            events.push(d.author(
                Kind::ENTITY_MERGE,
                None,
                None,
                enc(&EntityMergeBody {
                    nonce: [i as u8; 16],
                    from,
                    into,
                }),
                1_700_000_000_000 + i as u64 * 1000,
            ));
        }
        let p = fold(&events);
        assert_eq!(p.entity_of(a), c);
        assert_eq!(p.entity_of(b), c);
        assert_eq!(p.entity_of(c), c, "the end of a chain is itself");
    }

    #[test]
    fn merging_is_idempotent_and_order_independent() {
        let mut d = testing::Origin::new(0);
        let once = d.author(
            Kind::ENTITY_MERGE,
            None,
            None,
            enc(&EntityMergeBody {
                nonce: [1; 16],
                from: sarah(),
                into: chen(),
            }),
            1_700_000_000_000,
        );
        let twice = d.author(
            Kind::ENTITY_MERGE,
            None,
            None,
            enc(&EntityMergeBody {
                nonce: [2; 16],
                from: sarah(),
                into: chen(),
            }),
            1_700_000_100_000,
        );

        assert_eq!(
            fold(core::slice::from_ref(&once)).entity_of(sarah()),
            chen()
        );
        assert_eq!(
            fold(&[once.clone(), twice.clone()]).entity_of(sarah()),
            chen(),
            "the same merge twice is not the same as merging once"
        );
        assert_eq!(fold(&[twice, once]).entity_of(sarah()), chen());
    }

    #[test]
    fn merging_an_entity_into_itself_says_nothing() {
        let mut d = testing::Origin::new(0);
        let e = d.author(
            Kind::ENTITY_MERGE,
            None,
            None,
            enc(&EntityMergeBody {
                nonce: [1; 16],
                from: sarah(),
                into: sarah(),
            }),
            1_700_000_000_000,
        );
        let p = fold(&[e]);
        assert!(p.entity_merged.is_empty());
        assert_eq!(p.entity_of(sarah()), sarah());
    }

    #[test]
    fn a_merge_cycle_leaves_every_entity_reachable() {
        // Two devices, partitioned, each merging the other way. Neither is wrong
        // from where it stands, and refusing both would leave every read that
        // follows the pointer spinning or giving up — so an entity somebody
        // merged would become unreachable, which is losing it.
        let mut d = testing::Origin::new(0);
        let there = d.author(
            Kind::ENTITY_MERGE,
            None,
            None,
            enc(&EntityMergeBody {
                nonce: [1; 16],
                from: sarah(),
                into: chen(),
            }),
            1_700_000_000_000,
        );
        let back = d.author(
            Kind::ENTITY_MERGE,
            None,
            None,
            enc(&EntityMergeBody {
                nonce: [2; 16],
                from: chen(),
                into: sarah(),
            }),
            1_700_000_100_000,
        );

        let p = fold(&[there.clone(), back.clone()]);
        // Terminates, and lands somewhere real.
        let a = p.entity_of(sarah());
        let b = p.entity_of(chen());
        assert!(a == sarah() || a == chen());
        assert!(b == sarah() || b == chen());

        // And both devices agree, whichever order they saw them in.
        let q = fold(&[back, there]);
        assert_eq!(p.entity_of(sarah()), q.entity_of(sarah()));
        assert_eq!(p.entity_of(chen()), q.entity_of(chen()));
    }

    #[test]
    fn a_bind_needs_the_note_and_unbinding_does_not() {
        let mut d = testing::Origin::new(0);
        let note = d.author(
            Kind::CAPTURE,
            None,
            None,
            testing::capture_body(1, "the acme migration, as a project"),
            1_700_000_000_000,
        );
        let note_id = note.header.id();

        // Binding a note nobody holds is held, not guessed at — it may simply
        // not have arrived yet.
        let mut e = testing::Origin::new(1);
        let orphan = e.author(
            Kind::ENTITY_BIND,
            None,
            None,
            enc(&EntityBindBody {
                nonce: [1; 16],
                ent: sarah(),
                note: Some(EventId([0x77; 32])),
            }),
            1_700_000_001_000,
        );
        let orphan_id = orphan.header.id();
        let p = fold(&[note.clone(), orphan]);
        assert_eq!(p.held.get(&orphan_id), Some(&Held::NoTarget));
        assert_eq!(p.entity_backing_of(sarah()), None);

        // Binding one it does hold.
        let bound = e.author(
            Kind::ENTITY_BIND,
            None,
            None,
            enc(&EntityBindBody {
                nonce: [2; 16],
                ent: sarah(),
                note: Some(note_id),
            }),
            1_700_000_002_000,
        );
        let p = fold(&[note.clone(), bound.clone()]);
        assert_eq!(p.entity_backing_of(sarah()), Some(note_id));

        // And unbinding, which carries `None` and must beat the earlier bind
        // rather than being skipped as an absence.
        let unbound = e.author(
            Kind::ENTITY_BIND,
            None,
            None,
            enc(&EntityBindBody {
                nonce: [3; 16],
                ent: sarah(),
                note: None,
            }),
            1_700_000_003_000,
        );
        let p = fold(&[note, bound, unbound]);
        assert_eq!(p.entity_backing_of(sarah()), None, "unbinding was skipped");
    }

    #[test]
    fn a_renamed_entity_answers_through_its_merge() {
        // The two mechanisms together: B is renamed, A is merged into B, and
        // asking A for a name has to walk the merge before reading the display.
        let mut d = testing::Origin::new(0);
        let rename = d.author(
            Kind::ENTITY_RENAME,
            None,
            None,
            enc(&EntityRenameBody {
                nonce: [1; 16],
                ent: chen(),
                display: "Sarah Chen".into(),
            }),
            1_700_000_000_000,
        );
        let merge = d.author(
            Kind::ENTITY_MERGE,
            None,
            None,
            enc(&EntityMergeBody {
                nonce: [2; 16],
                from: sarah(),
                into: chen(),
            }),
            1_700_000_100_000,
        );
        let p = fold(&[rename, merge]);
        assert_eq!(p.entity_name(sarah()), Some("Sarah Chen"));
    }
}
