//! Deciding what to ask for.
//!
//! The correctness mechanism, and deliberately the dullest thing in the crate:
//! each side states what it holds, and each side works out for itself what it
//! is missing. There is no negotiation round, because a negotiation is a
//! protocol with states, and states are where a sync that quietly stops
//! working lives.
//!
//! # Why this is a pure function
//!
//! Everything here is a function of two version vectors. That means the
//! interesting cases — a device that has been off for a month, an origin
//! neither side authored, a peer that is ahead in one origin and behind in
//! another — are ordinary test inputs rather than situations to be reproduced
//! with real devices and real time.

use alloc::string::String;
use alloc::vec::Vec;

use capture_core::EventId;
use capture_core::envelope::Envelope;

use alloc::collections::BTreeSet;

use crate::message::{Message, Seal, VersionVector};
use crate::session::Session;
use crate::transport::{Transport, TransportError};

/// What this side is missing, as the messages that ask for it.
///
/// One request per origin the peer is ahead in. Ranges start at the successor
/// of what is held, never at an arbitrary point: a version vector cannot
/// represent a hole (§5.3), so asking for anything other than the next
/// sequence number would produce one — and a replica with a hole re-requests
/// the same range from every peer for ever.
///
/// Nothing is asked for on behalf of a *peer*. The vector names origins, and
/// an origin is not the device that hands it over — which is what makes any
/// device able to relay any other's events, and what makes "no device is
/// required to be online" true rather than aspirational.
pub fn wanted(mine: &VersionVector, theirs: &VersionVector) -> Vec<Message> {
    let mut out = Vec::new();
    for (dev, &head) in theirs {
        let held = mine.get(dev).copied().unwrap_or(0);
        if head > held {
            out.push(Message::Range {
                dev: String::from(dev),
                from: held + 1,
                to: head,
            });
        }
    }
    out
}

/// Which peers to sync with this round.
///
/// Fanout two or three, per §5.5. With two devices this degenerates to a full
/// mesh and the selection does nothing — which is exactly why it is written
/// now: "sync with the other one" is an assumption that works until a third
/// device arrives, and by then it is load-bearing in several places.
///
/// Rotating rather than random. A random choice needs a source of randomness
/// in a loop that has no other use for one, and rotation gives the property
/// that actually matters — that no peer is starved — while being a function of
/// a counter, which is a thing a test can hold still.
pub fn select(peers: &[[u8; 32]], round: u64, fanout: usize) -> Vec<[u8; 32]> {
    if peers.is_empty() {
        return Vec::new();
    }
    let take = fanout.min(peers.len());
    let start = (round as usize) % peers.len();
    (0..take)
        .map(|i| peers[(start + i) % peers.len()])
        .collect()
}

/// The fanout §5.5 names.
pub const FANOUT: usize = 3;

/// What the protocol needs from a store, and nothing else.
///
/// Narrow on purpose. This crate never folds, never decides what an event
/// means, and never touches the projection — it moves verified envelopes
/// across a wire and hands them over. The boundary is where an unreproducible
/// bug becomes a reproducible one, so it is a trait with an in-memory
/// implementation in the tests rather than a direct call into `capture-store`.
pub trait Archive {
    /// One sequence number per origin, gap-free.
    ///
    /// Sealed origins are in here too. Leaving them out is the mistake §5.5
    /// names: a newcomer would ask nothing about them, receive none of their
    /// events, and report agreement with every peer.
    fn heads(&self) -> VersionVector;

    /// Origins that will never author again, and the roster each was sealed
    /// against.
    ///
    /// Empty by default, because a replica that seals nothing is correct and
    /// merely does more work.
    fn sealed(&self) -> Vec<Seal> {
        Vec::new()
    }

    /// The cue types this device can actually watch for (§7.5).
    ///
    /// Empty by default, which is the honest answer for a replica that has
    /// never been asked rather than a claim to watch nothing.
    fn observes(&self) -> Vec<String> {
        Vec::new()
    }

    /// A digest of this replica's current roster, for comparing against the
    /// one a seal names.
    fn roster_digest(&self) -> [u8; 32] {
        [0u8; 32]
    }

    /// Every event this origin authored in the range, in sequence order.
    fn range(&self, dev: &str, from: u64, to: u64) -> Vec<Envelope>;

    /// Redactions this device still owes a peer, most urgent first.
    ///
    /// Handed over inside the ordinary exchange rather than in a phase of
    /// their own. A separate blocking round-trip deadlocks the moment both
    /// sides have one to send: each waits for an acknowledgement the other is
    /// not yet reading for.
    fn owed_redactions(&self, _peer: Option<[u8; 32]>) -> Vec<Envelope> {
        Vec::new()
    }

    /// The peer says it has folded this redaction.
    ///
    /// Only ever called on a real acknowledgement. "Gone everywhere" is the
    /// single claim a redaction makes, and recording it on send rather than on
    /// acknowledgement would make that claim false exactly when it matters.
    fn redaction_acked(&mut self, _id: EventId, _peer: Option<[u8; 32]>) {}

    /// Take an event that arrived from a peer.
    ///
    /// Fallible because ingest verifies: a signature that does not check out,
    /// a body that does not hash to its header, an origin that is not on the
    /// roster. The protocol does not decide what happens to those — it reports
    /// that one arrived and carries on, because a peer sending one bad event
    /// is not a reason to abandon the good ones behind it.
    fn ingest(&mut self, e: Envelope) -> bool;
}

/// What one exchange did.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Exchanged {
    /// Events taken in.
    pub received: usize,
    /// Events handed over.
    pub sent: usize,
    /// Events that arrived and were refused.
    pub refused: usize,
    /// Redactions the peer confirmed it had folded.
    pub redactions_acked: usize,
    /// What the peer said it can watch for (§7.5).
    ///
    /// Empty where it said nothing, which includes every build that predates
    /// the field. "I did not say" and "I can watch nothing" are the same
    /// shape here and the caller treats them the same way: it learns nothing
    /// and keeps whatever it knew before.
    pub peer_observes: Vec<String>,
}

/// Run one round of anti-entropy to completion.
///
/// Both sides run this, and it is symmetric: whoever dialled has no privileged
/// role, because the device that opened the connection is not necessarily the
/// one that is behind. A version that only pushed would leave a device that
/// never dials permanently stale.
///
/// # What this asks of a transport
///
/// Both sides write before either reads, so a transport whose `send` blocks
/// when the peer is not reading can deadlock here — which a TCP socket's send
/// buffer will do on a large enough round. The in-memory transport cannot
/// exhibit it, so it is written down rather than discovered: the socket
/// implementation owes either a writer thread or a non-blocking send, and
/// that is a requirement of this function rather than an implementation
/// detail of that one.
pub fn exchange<T: Transport, A: Archive>(
    session: &mut Session<T>,
    archive: &mut A,
) -> Result<Exchanged, TransportError> {
    let mine = archive.heads();
    let my_seals = archive.sealed();
    debug_assert!(
        my_seals.iter().all(|s| mine.contains_key(&s.dev)),
        "a sealed origin is missing from the version vector, which is the \
         completeness-oracle failure in §5.5: a newcomer asks nothing about \
         it, receives none of its events, and reports agreement with every \
         peer"
    );
    send(
        session,
        &Message::Hello {
            v: crate::message::PROTOCOL_VERSION,
            vv: mine.clone(),
            partial: false,
            sealed: if my_seals.is_empty() {
                None
            } else {
                Some(my_seals.clone())
            },
            observes: match archive.observes() {
                // Nothing to say is said by saying nothing, so the field
                // elides and the frame is byte-identical to one an older
                // build would have written.
                v if v.is_empty() => None,
                v => Some(v),
            },
        },
    )?;

    let (theirs, their_seals, peer_observes) = match decode(&session.recv_blocking()?) {
        Some(Message::Hello {
            vv,
            sealed,
            observes,
            ..
        }) => (vv, sealed.unwrap_or_default(), observes.unwrap_or_default()),
        // Anything else here is a peer speaking a protocol this build does not
        // know. There is nothing to negotiate down to, so the round is simply
        // over.
        _ => return Err(TransportError::Malformed),
    };

    // A sealed origin both sides agree on is settled, and settled for ever:
    // no event can ever be added to it. Skipping it is the only thing sealing
    // actually saves, and it saves nothing on a first sync — which is correct,
    // because a first sync is exactly when the events are needed.
    let settled = agreed(&my_seals, &their_seals, archive.roster_digest());
    // Before anything else this side asks for. For every other event arriving
    // late costs nothing and anti-entropy is the mechanism; for a redaction
    // the delivery window is exactly the window in which the content can still
    // be read, which is the thing the feature exists to close (§5.4).
    let peer = session.peer();
    for e in archive.owed_redactions(peer) {
        send(session, &Message::Redact(e))?;
    }

    for ask in wanted(&mine, &theirs) {
        if let Message::Range { dev, .. } = &ask
            && settled.contains(dev.as_str())
        {
            continue;
        }
        send(session, &ask)?;
    }
    send(session, &Message::Asked)?;

    let mut out = Exchanged {
        // Carried out of the Hello, which is the only place it is said.
        peer_observes,
        ..Exchanged::default()
    };
    // Two conditions, tracked separately, because a round has two phases per
    // side and they interleave on the wire: this side stops when the peer has
    // sent everything owed *and* this side has answered everything asked.
    // Collapsing them into one flag is how a side hangs up while the other is
    // still talking, which is what the first version of this did.
    let mut they_finished = false;
    let mut i_finished = false;

    while !(they_finished && i_finished) {
        match decode(&session.recv_blocking()?) {
            Some(Message::Range { dev, from, to }) => {
                for e in archive.range(&dev, from, to) {
                    send(session, &Message::Event(e))?;
                    out.sent += 1;
                }
            }
            Some(Message::Event(e)) => {
                if archive.ingest(e) {
                    out.received += 1;
                } else {
                    out.refused += 1;
                }
            }
            Some(Message::Redact(e)) => {
                // Acknowledged only after the store has taken it. An
                // acknowledgement sent on receipt would report a destruction
                // that has not happened.
                let id = e.header.id();
                if archive.ingest(e) {
                    out.received += 1;
                    send(session, &Message::RedactAck { id })?;
                } else {
                    out.refused += 1;
                }
            }
            Some(Message::RedactAck { id }) => {
                archive.redaction_acked(id, peer);
                out.redactions_acked += 1;
            }
            Some(Message::Asked) => {
                // No more requests are coming, and every one that did arrive
                // was answered above — requests precede this marker by
                // construction.
                send(session, &Message::Done)?;
                i_finished = true;
            }
            Some(Message::Done) => {
                they_finished = true;
            }
            // A message this build has no rule for. Ignored rather than
            // fatal: an older peer must be able to talk to a newer one, and
            // hanging up on an unknown variant makes every future addition a
            // breaking change.
            _ => {}
        }
    }

    Ok(out)
}

/// Origins both sides have sealed identically, under a roster both agree is
/// current.
///
/// A seal computed against a different roster is ignored rather than trusted:
/// the roster is concurrently mutable, and a seal computed while a device was
/// being admitted may not account for what that device holds. Ignoring it
/// costs a diff and cannot lose an event, which is the right direction for
/// the two to fail in.
fn agreed<'a>(mine: &'a [Seal], theirs: &'a [Seal], roster: [u8; 32]) -> BTreeSet<&'a str> {
    let mut out = BTreeSet::new();
    for m in mine {
        if m.roster != roster {
            continue;
        }
        if theirs
            .iter()
            .any(|t| t.dev == m.dev && t.seq == m.seq && t.last == m.last && t.roster == roster)
        {
            out.insert(m.dev.as_str());
        }
    }
    out
}

fn send<T: Transport>(s: &mut Session<T>, m: &Message) -> Result<(), TransportError> {
    let mut b = Vec::new();
    minicbor::encode(m, &mut b).expect("encoding into a Vec is infallible");
    s.send(&b)
}

fn decode(b: &[u8]) -> Option<Message> {
    minicbor::decode(b).ok()
}
