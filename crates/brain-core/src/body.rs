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

use crate::{Nonce, id::EventId};

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
}

/// Helper: build a body prefix from a supplied nonce.
///
/// `brain-core` cannot generate one — entropy needs a syscall — so this exists
/// to make the boundary obvious at every call site.
pub const fn nonce_bytes(n: Nonce) -> [u8; 16] {
    n.0
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
