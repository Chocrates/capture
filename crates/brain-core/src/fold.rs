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
        AliasBody, CaptureBody, EditBody, MuteBody, MuteScope, OccBody, OccEditBody, OccMoveBody,
        Outcome, PinBody, PromoteBody, RedactBody, RedactCancelBody, ResolveBody, ReviewBody,
        SettingBody, TagBody, UntagBody,
    },
    envelope::Envelope,
    event::Kind,
    hlc::Hlc,
    id::{Dev, EventId},
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
    /// What review has done with this note.
    ///
    /// Counts of engagement, and nothing about memory. §2.7 withdrew the graded
    /// outcome deliberately: a recurring, permanently-recorded record of recall
    /// failures, on a schedule the user did not set, is the single most harmful
    /// thing this design could contain for the person it is for.
    #[n(22)]
    pub review: ReviewState,
    /// Per-occurrence exceptions to this note's series, keyed by original start.
    ///
    /// Only ever populated for a promoted series. An exception naming an
    /// occurrence the rule does not produce is kept rather than discarded: the
    /// rule can be edited later, and a completion that vanished because the
    /// schedule moved would be a completion silently lost.
    #[n(21)]
    pub occurrences: BTreeMap<i64, OccState>,
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
}

impl Note {
    /// Whether the note has been redacted.
    pub fn is_redacted(&self) -> bool {
        self.redacted_at.is_some()
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

impl ReviewState {
    /// Whether review should stop offering this.
    pub fn declined(&self) -> bool {
        self.deferred >= 2
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
    /// Skipped. Retained but inert when the occurrence has also been moved —
    /// see [`fold`] for why a move outranks a cancellation.
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
}

impl OccState {
    /// Whether this occurrence should be drawn.
    ///
    /// A cancelled occurrence is hidden, not gone: the cancellation is an
    /// ordinary event and nothing has been destroyed. A moved one is drawn at
    /// its new time even if something also cancelled it.
    pub fn visible(&self) -> bool {
        self.moved_to.is_some() || !self.cancelled
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
}

impl Projection {
    /// Canonical bytes. Two folds of the same events encode identically.
    pub fn canonical(&self) -> Vec<u8> {
        let mut b = Vec::new();
        minicbor::encode(self, &mut b).expect("encoding into Vec is infallible");
        b
    }
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
    let mut mutes: Vec<(MuteScope, Option<u64>)> = Vec::new();

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
                    continue;
                };
                // Capped. An uncapped redaction lets one loop destroy the whole
                // archive everywhere within a single push round.
                for t in r.targets.iter().take(crate::body::REDACT_TARGET_CAP) {
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
            Kind::OCC_MOVE => apply_occ_move(&mut proj, e),
            Kind::OCC_EDIT => apply_occ_edit(&mut proj, e),
            Kind::OCC_CANCEL => apply_occ(&mut proj, e, |o| o.cancelled = true),
            Kind::OCC_DONE => apply_occ(&mut proj, e, |o| o.done = true),
            Kind::OCC_UNDONE => apply_occ(&mut proj, e, |o| o.done = false),
            Kind::INTENT => apply_intent(&mut proj, e),
            Kind::ALIAS => apply_alias(&mut proj, &mut names, e),
            Kind::UNFLAG => apply_unflag(&mut proj, e),
            Kind::RESOLVE => collect_resolve(&mut proj, &mut dismissed, e),
            Kind::MUTE => collect_mute(&mut proj, &mut mutes, e),
            Kind::TOMBSTONE => apply_tombstone(&mut proj, e),
            // Membership and settings do not affect note state.
            Kind::SETTING => apply_setting(&mut proj, e),
            Kind::DEVICE_ADD | Kind::ORIGIN_CLAIM => {}
            // Handled outside the main pass.
            Kind::REDACT | Kind::REDACT_CANCEL => {}
            _ => {
                proj.held.insert(e.header.id(), Held::UnknownKind);
            }
        }
    }

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
    let date = "1970-01-01";
    let s = match text.as_deref() {
        Some(t) => slug::derive(t, date, &tgt),
        None => slug::derive_redacted(&tgt),
    };

    proj.notes.insert(
        tgt,
        Note {
            tgt,
            slug: s.text,
            alias: None,
            alias_at: None,
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
            review: ReviewState::default(),
            occurrences: BTreeMap::new(),
            muted_until: None,
            redacted_at: None,
            redaction_pending: None,
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

    let wins = match &n.promotion {
        None => true,
        Some(current) => match (current.pinned, incoming.pinned) {
            // A human correction stands, however late the machine is.
            (true, false) => false,
            // A person overrules the parser whenever they say so.
            (false, true) => true,
            // Equal authority: ordinary last-write-wins.
            _ => incoming.at > current.at,
        },
    };

    if wins {
        n.promotion = Some(incoming);
    }
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
fn collect_mute(proj: &mut Projection, mutes: &mut Vec<(MuteScope, Option<u64>)>, e: &Envelope) {
    let Some(body) = e.body.as_ref() else { return };
    let Ok(m) = minicbor::decode::<MuteBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    mutes.push((m.scope, m.until));
}

/// Work out which notes are suppressed.
///
/// Applied after everything else, in timestamp order, so the newest mute
/// touching a note wins. A mute is not a property of the note it was written
/// against — it is a rule, and the notes it covers are whichever ones match
/// when the question is asked.
fn apply_mutes(proj: &mut Projection, mutes: &[(MuteScope, Option<u64>)]) {
    for (scope, until) in mutes {
        // Zero means indefinite, which is distinct from absent: a note muted
        // forever and a note that was never muted look the same otherwise.
        let value = Some(until.unwrap_or(0));

        match scope {
            MuteScope::Note(tgt) => {
                if let Some(n) = proj.notes.get_mut(tgt) {
                    n.muted_until = value;
                }
            }
            MuteScope::Tag(tag) => {
                for n in proj.notes.values_mut() {
                    if n.tags.contains(tag) {
                        n.muted_until = value;
                    }
                }
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
                    }
                }
            }
        }
    }
}

/// The UTC calendar day a note was captured on.
///
/// **In UTC, not in the note's own timezone**, and that is a real limitation
/// rather than an oversight. `brain-core` holds no timezone database and must
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
}
