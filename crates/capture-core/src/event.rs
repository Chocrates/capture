//! The event envelope. **Frozen.**
//!
//! Every field below is hashed into [`EventId`] and therefore into the
//! signature. None of them can change meaning, move, or be removed. Fields may
//! be *added* at a new index; that is the only permitted evolution.

use minicbor::{Decode, Encode};

use crate::{
    hlc::Hlc,
    id::{BodyHash, Dev, EventId},
};

/// Envelope shape version.
///
/// Not a body version — bodies are frozen per kind, and a breaking change to a
/// body takes a **new kind name** instead. This exists only for the rare case
/// where the envelope itself must change, which is a deliberate, converted fork.
pub const ENVELOPE_VERSION: u16 = 1;

/// What an event is.
///
/// A `u16` rather than a closed enum, deliberately: an event of an unrecognised
/// kind must be *storable*. A device two versions behind still has to accept,
/// verify, store and relay events it cannot interpret — dropping them would
/// leave a sequence gap, and a version vector cannot represent a hole, so
/// anti-entropy would re-request it forever.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Encode, Decode)]
#[cbor(transparent)]
pub struct Kind(#[n(0)] pub u16);

impl Kind {
    /// Raw input. Never rejected, never validated.
    pub const CAPTURE: Kind = Kind(1);
    /// Human mutation of a note's body.
    pub const EDIT: Kind = Kind(2);
    /// Add a tag. Membership is an add-wins observed-remove set.
    pub const TAG: Kind = Kind(3);
    /// Retract specific tag events this device observed.
    pub const UNTAG: Kind = Kind(4);
    /// Assign a chosen human-readable name.
    pub const ALIAS: Kind = Kind(5);
    /// Pin a note.
    pub const PIN: Kind = Kind(6);
    /// Suppress from review and pushed surfaces. Reversible, unlike redaction.
    pub const MUTE: Kind = Kind(7);
    /// Dismiss a recorded divergence.
    pub const RESOLVE: Kind = Kind(8);
    /// Destroy content on every replica. Irreversible.
    pub const REDACT: Kind = Kind(9);
    /// Abort a queued redaction inside its window.
    pub const REDACT_CANCEL: Kind = Kind(10);
    /// Soft delete. Hides; loses to a concurrent edit.
    pub const TOMBSTONE: Kind = Kind(11);
    /// Admit a public key to the roster.
    pub const DEVICE_ADD: Kind = Kind(12);
    /// An existing roster key declaring a new origin for itself after a restore.
    ///
    /// Without this, a restored device's events reference an origin that maps to
    /// no key, and every peer refuses them permanently — restore becomes a
    /// lockout, discovered only once a second device exists.
    pub const ORIGIN_CLAIM: Kind = Kind(13);
    /// A user preference. Absent means the compiled-in default, which is how
    /// "did I choose this, or is it just the default?" stays answerable.
    pub const SETTING: Kind = Kind(14);

    /// Clear a note's credential flag.
    ///
    /// The flag is derived from the capture text by a predicate that
    /// deliberately over-flags, and it keeps the note out of exports. An
    /// over-flagged note would otherwise be quietly missing from every backup,
    /// so clearing the flag has to be one gesture — and, being a claim about a
    /// note that every replica must agree on, an event rather than local state.
    pub const UNFLAG: Kind = Kind(15);

    /// Assigns a derived type — event, task, or recurring series — with its
    /// fields (§3.1). Reversible; a later `promote` supersedes it.
    pub const PROMOTE: Kind = Kind(16);
    /// An if-then plan: a cue specification and an action (§2.4).
    pub const INTENT: Kind = Kind(17);
    /// A scheduled resurfacing happened, and what became of it (§2.7).
    pub const REVIEW: Kind = Kind(18);

    // Single-occurrence exceptions to a recurring series, keyed by
    // `(tgt, occ)` (§3.5). Each affects exactly one occurrence and leaves the
    // rule untouched.

    /// This occurrence only: new start and end.
    pub const OCC_MOVE: Kind = Kind(19);
    /// This occurrence only: different title, location or notes.
    pub const OCC_EDIT: Kind = Kind(20);
    /// This occurrence only: skipped. The series continues.
    pub const OCC_CANCEL: Kind = Kind(21);
    /// Completion of one instance of a recurring obligation (§3.6).
    pub const OCC_DONE: Kind = Kind(22);
    /// Un-completing a mistaken tick. Not a pure union, deliberately.
    pub const OCC_UNDONE: Kind = Kind(23);
    /// Completion of one item of a list capture (§3.6).
    ///
    /// The list's twin of `OCC_DONE`, and for the same reason: an item is a
    /// projection row expanded from a capture, not a thing in the log, so
    /// completing one is a fact *about* that capture rather than a new
    /// capture. Minting a note per completed item would fill the timeline
    /// with entries nobody wrote.
    pub const ITEM_DONE: Kind = Kind(28);
    /// Un-completing a mistaken tick. Not a pure union, deliberately — the
    /// same argument as `OCC_UNDONE`.
    pub const ITEM_UNDONE: Kind = Kind(29);
    /// Puts one note under another in the wiki, or back at the top.
    ///
    /// The one place this design files something by hand. §3.7 argues at
    /// length that structure should be derived — a hierarchy somebody
    /// maintains is a hierarchy that stops being maintained, and a note filed
    /// in the wrong branch is a note that is gone. It is here because it was
    /// asked for after that argument was put, and the cost is real and
    /// bounded: nothing else in the system reads it, so a wrong parent misfiles
    /// a page in one sidebar and loses nothing.
    pub const PARENT: Kind = Kind(24);
    /// The secret every roster member shares, and nobody else holds.
    ///
    /// It keys the discovery pseudonym and the version-vector digest (§5.2),
    /// which are broadcast in the clear on every network the device joins.
    /// Members can recognise each other in that broadcast; an observer sees a
    /// value that changes per network and on a schedule, and can correlate
    /// nothing from it.
    ///
    /// Minting and rotating are the same event, so the first key is simply the
    /// first rotation and there is one code path rather than two. Rotation is
    /// the reason this is an explicit secret rather than something derived
    /// from the member set: a retired device holds the whole log, so anything
    /// computable from what it already has stays computable by it, and the
    /// device you retired because it was stolen goes on recognising your
    /// devices on every network it meets them. It cannot receive the
    /// replacement, because ingesting the retirement tears its sessions down
    /// (§5.3).
    pub const ROSTER_KEY: Kind = Kind(25);

    /// Revoke a key's membership, naming the last event of each of its
    /// origins that is to be honoured.
    ///
    /// A head rather than a timestamp. The obvious rule — reject what the
    /// retired device authored "after" the retirement — cannot work, because
    /// that device chooses its own `hlc` and can simply backdate; there is no
    /// causal ordering between a retirement and the events of the device being
    /// retired. The per-origin hash chain makes a head enforceable instead:
    /// every event names its predecessor, so anything beyond the named one is
    /// provably outside the retired history whatever timestamp it claims.
    ///
    /// It retires the **key**, not an origin. Retiring an origin would let a
    /// stolen device mint a fresh incarnation and walk straight back in, which
    /// would make the whole remedy evaporate on one restart.
    pub const DEVICE_RETIRE: Kind = Kind(26);

    /// Re-admit a key, signed by two.
    ///
    /// The only event whose validity does not descend from current roster
    /// membership, and that exception is stated rather than left implicit.
    ///
    /// It exists because any member may retire any member, so one compromised
    /// or malfunctioning device can retire every other — after which no honest
    /// device can re-admit anything, since `device_add` requires a signature
    /// from a current member and there are none left. Pairing does not rescue
    /// it either: pairing bootstraps transport keys, while the roster entry
    /// that gates ingest still has to come from a member. Both devices would
    /// sit there able to talk and unable to accept each other.
    ///
    /// Forging one requires compromising two devices *and* being in the room
    /// with them, which is a materially higher bar than the single compromised
    /// device that caused the lockout.
    pub const DEVICE_READMIT: Kind = Kind(27);

    /// Whether this build understands the kind.
    ///
    /// An unknown kind is quarantined — stored, verified, relayed, not folded —
    /// and re-evaluated after an upgrade.
    pub fn is_known(&self) -> bool {
        (1..=29).contains(&self.0)
    }
}

/// The signed part of an event.
///
/// `id = blake3(canonical(Header))`, and `sig` is Ed25519 over that id. The body
/// appears here only as [`BodyHash`], which is why a redacted event still
/// recomputes and still verifies — and why `blake3(body) == bhash` has to be
/// checked separately at ingest.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Encode, Decode)]
pub struct Header {
    /// Envelope shape version.
    #[n(0)]
    pub v: u16,
    /// Authoring origin. Never "the device that owns the note" — there is no
    /// such thing; any device may act on any note.
    #[n(1)]
    pub dev: Dev,
    /// Per-origin, gap-free, monotonic.
    #[n(2)]
    pub seq: u64,
    /// Total order across all origins.
    #[n(3)]
    pub hlc: Hlc,
    /// What kind of event this is.
    #[n(4)]
    pub kind: Kind,
    /// **Which note.** The id of the root capture. Set by every event that acts
    /// on a note, and indexed, so folding one note is a single lookup rather
    /// than a walk back up a chain.
    #[n(5)]
    pub tgt: Option<EventId>,
    /// **Which version.** The exact version being superseded. Exists solely to
    /// tell a sequential edit from a genuinely concurrent one. Null on a
    /// capture, where `tgt` is the event's own id.
    #[n(6)]
    pub prev: Option<EventId>,
    /// Recurrence exceptions: the occurrence's original start.
    #[n(7)]
    pub occ: Option<u64>,
    /// This origin's previous event. Gives each origin a verifiable hash chain,
    /// independent of any timestamp the origin asserts about itself.
    #[n(8)]
    pub chain: Option<EventId>,
    /// Commitment to the canonical body.
    #[n(9)]
    pub bhash: BodyHash,
}

impl Header {
    /// Compute this header's event id.
    pub fn id(&self) -> EventId {
        let mut buf = alloc::vec::Vec::new();
        minicbor::encode(self, &mut buf).expect("header encoding is infallible into Vec");
        EventId(*blake3::hash(&buf).as_bytes())
    }

    /// Canonical bytes, for signing and for golden vectors.
    pub fn canonical(&self) -> alloc::vec::Vec<u8> {
        let mut buf = alloc::vec::Vec::new();
        minicbor::encode(self, &mut buf).expect("header encoding is infallible into Vec");
        buf
    }
}

#[cfg(test)]
mod m2_kinds {
    use super::*;

    #[test]
    fn the_new_kinds_are_known() {
        for k in [
            Kind::PROMOTE,
            Kind::INTENT,
            Kind::REVIEW,
            Kind::OCC_MOVE,
            Kind::OCC_EDIT,
            Kind::OCC_CANCEL,
            Kind::OCC_DONE,
            Kind::ITEM_DONE,
            Kind::ITEM_UNDONE,
            Kind::OCC_UNDONE,
            Kind::PARENT,
        ] {
            assert!(k.is_known(), "{k:?} was added and not admitted");
        }
    }

    #[test]
    fn kind_numbers_are_permanent() {
        // Written down so that inserting a kind in the middle — which would
        // silently reinterpret every event already written under the numbers it
        // displaced — fails here instead of in somebody's archive.
        assert_eq!(Kind::PROMOTE.0, 16);
        assert_eq!(Kind::INTENT.0, 17);
        assert_eq!(Kind::REVIEW.0, 18);
        assert_eq!(Kind::OCC_MOVE.0, 19);
        assert_eq!(Kind::OCC_EDIT.0, 20);
        assert_eq!(Kind::OCC_CANCEL.0, 21);
        assert_eq!(Kind::OCC_DONE.0, 22);
        assert_eq!(Kind::OCC_UNDONE.0, 23);
        assert_eq!(Kind::PARENT.0, 24);
        assert_eq!(Kind::ROSTER_KEY.0, 25);
        assert_eq!(Kind::DEVICE_RETIRE.0, 26);
        assert_eq!(Kind::DEVICE_READMIT.0, 27);
        assert_eq!(Kind::ITEM_DONE.0, 28);
        assert_eq!(Kind::ITEM_UNDONE.0, 29);
    }

    #[test]
    fn a_kind_from_a_later_build_is_still_unknown() {
        // The boundary has to keep working after it moves. An event written by
        // a build that knows one more kind than this one must be held rather
        // than discarded or misread as something it knows.
        assert!(!Kind(30).is_known());
        assert!(!Kind(0).is_known(), "zero is not a kind");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Incarnation;

    fn header() -> Header {
        Header {
            v: ENVELOPE_VERSION,
            dev: Dev::derive(&[1u8; 32], &Incarnation([2u8; 16])),
            seq: 41822,
            hlc: Hlc {
                wall: 1_755_710_241_371,
                counter: 3,
                origin: Dev([9u8; 32]),
            },
            kind: Kind::CAPTURE,
            tgt: None,
            prev: None,
            occ: None,
            chain: None,
            bhash: BodyHash::of(b"body"),
        }
    }

    #[test]
    fn encoding_is_stable_across_runs() {
        let h = header();
        assert_eq!(h.canonical(), h.canonical());
        assert_eq!(h.id(), h.id());
    }

    #[test]
    fn every_header_field_changes_the_id() {
        let base = header();
        let id = base.id();

        let mut h = base;
        h.seq += 1;
        assert_ne!(h.id(), id, "seq must be covered");

        let mut h = base;
        h.kind = Kind::EDIT;
        assert_ne!(
            h.id(),
            id,
            "kind must be covered — else a body can be replayed as a different kind"
        );

        let mut h = base;
        h.tgt = Some(EventId([3u8; 32]));
        assert_ne!(
            h.id(),
            id,
            "tgt must be covered — else an edit can be retargeted at any note"
        );

        let mut h = base;
        h.dev = Dev([4u8; 32]);
        assert_ne!(h.id(), id, "dev must be covered");

        let mut h = base;
        h.hlc.counter += 1;
        assert_ne!(h.id(), id, "hlc must be covered");

        let mut h = base;
        h.bhash = BodyHash::of(b"different");
        assert_ne!(h.id(), id, "the body commitment must be covered");
    }

    #[test]
    fn unknown_kinds_are_representable() {
        // A device two versions behind must be able to hold an event it cannot
        // interpret; dropping it would leave a gap the version vector cannot
        // express.
        let k = Kind(9999);
        assert!(!k.is_known());
        let mut h = header();
        h.kind = k;
        let _ = h.id(); // encodes without complaint
    }
}
