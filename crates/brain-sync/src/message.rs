//! What crosses the wire, and its encoding.
//!
//! # Frozen from the first write, like an event body
//!
//! A protocol message is as permanent as an event body the moment two versions
//! of this program exist in the world: one of them will be older, and it will
//! be decoding what the other wrote. So the encodings here get golden vectors
//! before anything sends one, exactly as `brain-core`'s bodies do — the same
//! discipline for the same reason.
//!
//! Field numbers are permanent. A field may be added at the end; none may be
//! renumbered, and none may change meaning.

use alloc::collections::BTreeMap;
use alloc::string::String;

use brain_core::EventId;
use brain_core::envelope::Envelope;
use minicbor::{Decode, Encode};

/// The version of this protocol.
///
/// Sent in `HELLO` so that two builds can discover a disagreement rather than
/// misread each other. Nothing negotiates on it yet — there is one version —
/// but a wire with no version has no way to ever gain one, which is the same
/// argument `ENVELOPE_VERSION` makes for events.
pub const PROTOCOL_VERSION: u16 = 1;

/// What a replica holds: one sequence number per origin.
///
/// Keyed by the origin's device identifier in hex, which is how `heads` in the
/// log already keys it. The whole reconciliation algorithm is a subtraction
/// over two of these (§5.5).
pub type VersionVector = BTreeMap<String, u64>;

/// One message.
///
/// The variants are numbered and the numbers are permanent.
#[derive(Clone, PartialEq, Eq, Debug, Encode, Decode)]
pub enum Message {
    /// Opening statement: what I am, and what I hold.
    #[n(0)]
    Hello {
        /// The protocol this side speaks.
        #[n(0)]
        v: u16,
        /// One sequence number per origin.
        #[n(1)]
        vv: VersionVector,
        /// This replica holds a window rather than everything (§5.5).
        ///
        /// Always false today: leaf replicas are not built, and a partial
        /// replica whose coverage nothing tests is a way to lose events
        /// quietly. The flag is on the wire from the first version all the
        /// same, because the failure it prevents is a future leaf being
        /// mistaken by an older build for a complete replica and used to seed
        /// a new device.
        #[n(2)]
        partial: bool,
    },

    /// Send me this origin's events from `from` to `to`, inclusive.
    #[n(1)]
    Range {
        /// The origin, in hex.
        #[n(0)]
        dev: String,
        #[n(1)]
        from: u64,
        #[n(2)]
        to: u64,
    },

    /// An event, pushed or answering a `RANGE`.
    #[n(2)]
    Event(#[n(0)] Envelope),

    /// A redaction, which is pushed eagerly rather than left to anti-entropy.
    ///
    /// A separate variant from `Event` on purpose. The two are the same shape
    /// and are treated differently — this one is retried until acknowledged —
    /// and a receiver has to be able to tell them apart without decoding the
    /// body to find out.
    #[n(3)]
    Redact(#[n(0)] Envelope),

    /// I have folded that redaction.
    #[n(4)]
    RedactAck {
        #[n(0)]
        id: EventId,
    },

    /// Send me every event targeting this note.
    ///
    /// For a replica that lacks a note and therefore cannot know which sequence
    /// ranges hold its events — precisely the index it does not have (§5.5).
    #[n(5)]
    WantTgt {
        #[n(0)]
        tgt: EventId,
    },

    /// That is everything I am going to ask for.
    ///
    /// Distinct from [`Message::Done`], and the distinction is the whole of
    /// the termination rule. A round has two phases per side — asking, then
    /// answering — and a side's answers necessarily come *after* its
    /// questions, so a single marker cannot mean both. The first version of
    /// this had one, and both sides hung up on reading it while the events
    /// they had asked for were still in flight behind it.
    #[n(6)]
    Asked,

    /// I have sent everything you asked me for.
    ///
    /// Termination could be inferred — each side knows how many events it
    /// asked for, because it named the ranges. It is stated instead, because
    /// inferring it means a peer that cannot produce one of them leaves the
    /// other blocked for ever waiting on a count that will never be reached,
    /// and "the sync hangs occasionally" is the least diagnosable failure this
    /// protocol could have.
    #[n(7)]
    Done,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn roundtrip(m: &Message) -> Message {
        let mut b = Vec::new();
        minicbor::encode(m, &mut b).expect("encoding into a Vec is infallible");
        minicbor::decode(&b).expect("what this crate wrote, it can read")
    }

    #[test]
    fn every_message_survives_the_wire() {
        let mut vv = VersionVector::new();
        vv.insert(String::from("aa"), 41822);
        vv.insert(String::from("bb"), 9310);

        for m in [
            Message::Hello {
                v: PROTOCOL_VERSION,
                vv: vv.clone(),
                partial: false,
            },
            Message::Range {
                dev: String::from("aa"),
                from: 41501,
                to: 41822,
            },
            Message::RedactAck {
                id: EventId([7; 32]),
            },
            Message::WantTgt {
                tgt: EventId([9; 32]),
            },
        ] {
            assert_eq!(roundtrip(&m), m, "{m:?} did not survive");
        }
    }

    #[test]
    fn a_version_vector_keeps_every_origin() {
        // The vector is the whole of what a peer knows about itself, and an
        // origin dropped from it is one whose events are never asked for.
        let mut vv = VersionVector::new();
        for i in 0..50u64 {
            vv.insert(alloc::format!("{i:02x}"), i * 7);
        }
        let m = Message::Hello {
            v: PROTOCOL_VERSION,
            vv: vv.clone(),
            partial: false,
        };
        match roundtrip(&m) {
            Message::Hello { vv: back, .. } => assert_eq!(back, vv),
            other => panic!("a hello came back as {other:?}"),
        }
    }

    #[test]
    fn a_leaf_says_so() {
        // The flag exists before leaves do, so an older build cannot mistake a
        // future partial replica for a complete one and seed a device from it.
        let m = Message::Hello {
            v: PROTOCOL_VERSION,
            vv: VersionVector::new(),
            partial: true,
        };
        match roundtrip(&m) {
            Message::Hello { partial, .. } => assert!(partial),
            other => panic!("a hello came back as {other:?}"),
        }
    }
}
