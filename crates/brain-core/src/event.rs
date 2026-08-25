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

    /// Whether this build understands the kind.
    ///
    /// An unknown kind is quarantined — stored, verified, relayed, not folded —
    /// and re-evaluated after an upgrade.
    pub fn is_known(&self) -> bool {
        (1..=15).contains(&self.0)
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
