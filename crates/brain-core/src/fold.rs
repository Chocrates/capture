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
    body::{CaptureBody, EditBody, MuteBody, MuteScope, RedactBody, TagBody, UntagBody},
    envelope::Envelope,
    event::Kind,
    hlc::Hlc,
    id::EventId,
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

/// A note's current state.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub struct Note {
    /// The note's identity — its first event.
    #[n(0)]
    pub tgt: EventId,
    /// Human-readable name.
    #[n(1)]
    pub slug: String,
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
    /// Current tags.
    #[n(7)]
    pub tags: BTreeSet<String>,
    /// Whether it is pinned.
    #[n(8)]
    pub pinned: bool,
    /// Suppressed from surfacing until this time. Zero means indefinitely.
    #[n(9)]
    pub muted_until: Option<u64>,
    /// When it was redacted, if it was.
    #[n(10)]
    pub redacted_at: Option<Hlc>,
    /// Independent versions the fold could not order by causality.
    #[n(11)]
    pub divergences: Vec<Divergence>,
    /// Every version, including excluded ones.
    #[n(12)]
    pub versions: Vec<Version>,
}

impl Note {
    /// Whether the note has been redacted.
    pub fn is_redacted(&self) -> bool {
        self.redacted_at.is_some()
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

/// Compute state from a set of events.
pub fn fold(events: &[Envelope]) -> Projection {
    // Sort by timestamp, not arrival. Everything downstream reads this order,
    // so the result cannot depend on how the events turned up.
    let mut ordered: Vec<&Envelope> = events.iter().collect();
    ordered.sort_by_key(|a| a.header.hlc);

    let mut proj = Projection::default();
    let mut tags: BTreeMap<EventId, Vec<LiveTag>> = BTreeMap::new();
    let mut texts: BTreeMap<EventId, String> = BTreeMap::new();

    // Redactions are collected first and applied last, because a redaction must
    // take effect whether it arrives before or after the thing it destroys. If
    // it were applied in timestamp order, an event that turned up afterwards
    // would simply be readable again — and "destroyed" would mean "destroyed
    // unless something is slow", which is not a guarantee.
    let mut redacted: BTreeMap<EventId, Hlc> = BTreeMap::new();
    for e in &ordered {
        if e.header.kind == Kind::REDACT
            && let Some(body) = &e.body
            && let Ok(r) = minicbor::decode::<RedactBody>(body)
        {
            for t in r.targets.iter().take(crate::body::REDACT_TARGET_CAP) {
                redacted.entry(*t).or_insert(e.header.hlc);
            }
        }
    }

    for e in &ordered {
        if !e.header.kind.is_known() {
            proj.held.insert(e.header.id(), Held::UnknownKind);
            continue;
        }

        match e.header.kind {
            Kind::CAPTURE => apply_capture(&mut proj, e),
            Kind::EDIT => apply_edit(&mut proj, &mut texts, e),
            Kind::TAG => apply_tag(&mut proj, &mut tags, e),
            Kind::UNTAG => apply_untag(&mut proj, &mut tags, e),
            Kind::PIN => apply_pin(&mut proj, e),
            Kind::MUTE => apply_mute(&mut proj, e),
            Kind::TOMBSTONE => apply_tombstone(&mut proj, e),
            // Membership and settings do not affect note state.
            Kind::DEVICE_ADD | Kind::ORIGIN_CLAIM | Kind::SETTING => {}
            // Handled outside the main pass.
            Kind::REDACT | Kind::REDACT_CANCEL | Kind::RESOLVE | Kind::ALIAS => {}
            _ => {
                proj.held.insert(e.header.id(), Held::UnknownKind);
            }
        }
    }

    resolve(&mut proj, &texts);

    // Now apply redactions, whatever order they arrived in.
    for (tgt, at) in &redacted {
        if let Some(n) = proj.notes.get_mut(tgt) {
            n.text = None;
            n.redacted_at = Some(*at);
            // The name is rebuilt from identity alone. Two jobs: the note stays
            // reconstructible on every future fold although its text is gone,
            // and its own name stops carrying the words it was built from.
            n.slug = slug::derive_redacted(tgt).text;
        }
    }

    proj
}

fn apply_capture(proj: &mut Projection, e: &Envelope) {
    let tgt = e.header.tgt.unwrap_or_else(|| e.header.id());
    let (text, tz) = match e.body.as_ref().map(|b| minicbor::decode::<CaptureBody>(b)) {
        Some(Ok(c)) => (Some(c.text), c.tz),
        _ => {
            proj.held.insert(e.header.id(), Held::Malformed);
            return;
        }
    };

    // The name derives from the capture, which never changes — not from the
    // current text, which does. Otherwise an edit would rename the note and
    // break every link written to it.
    let date = "1970-01-01";
    let s = slug::derive(text.as_deref().unwrap_or(""), date, &tgt);

    proj.notes.insert(
        tgt,
        Note {
            tgt,
            slug: s.text,
            text,
            head: e.header.id(),
            created: e.header.hlc,
            updated: e.header.hlc,
            tz,
            tags: BTreeSet::new(),
            pinned: false,
            muted_until: None,
            redacted_at: None,
            divergences: Vec::new(),
            versions: alloc::vec![Version {
                id: e.header.id(),
                prev: None,
                hlc: e.header.hlc,
                kind: Kind::CAPTURE,
                excluded: false,
            }],
        },
    );
}

fn apply_edit(proj: &mut Projection, texts: &mut BTreeMap<EventId, String>, e: &Envelope) {
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
        n.tags.insert(tag);
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
    }
}

fn apply_pin(proj: &mut Projection, e: &Envelope) {
    if let Some(tgt) = e.header.tgt
        && let Some(n) = proj.notes.get_mut(&tgt)
    {
        // A plain value with no history to reconcile: the later one wins, and
        // events are already in timestamp order.
        n.pinned = true;
    }
}

fn apply_mute(proj: &mut Projection, e: &Envelope) {
    let Some(body) = e.body.as_ref() else { return };
    let Ok(m) = minicbor::decode::<MuteBody>(body) else {
        proj.held.insert(e.header.id(), Held::Malformed);
        return;
    };
    if let MuteScope::Note(tgt) = m.scope
        && let Some(n) = proj.notes.get_mut(&tgt)
    {
        n.muted_until = Some(m.until.unwrap_or(0));
    }
}

fn apply_tombstone(proj: &mut Projection, e: &Envelope) {
    let Some(tgt) = e.header.tgt else { return };
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
    fn a_redaction_destroys_text_whatever_order_it_arrives_in() {
        use crate::body::{RedactBody, RedactScope};
        let s = testing::mixed(8, 1, 1, 2);
        let tgt = s.notes[0];

        let mut d = testing::Origin::new(5);
        let body = testing::encode(&RedactBody {
            nonce: [0u8; 16],
            scope: RedactScope::WholeNote,
            targets: alloc::vec![tgt],
        });
        let r = d.author(Kind::REDACT, Some(tgt), None, body, 1_700_000_000_000);

        // Redaction last, and redaction first. Both must destroy.
        let mut after = s.events.clone();
        after.push(r.clone());
        let mut before = alloc::vec![r];
        before.extend(s.events.clone());

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
        let mut set = s.events.clone();
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
