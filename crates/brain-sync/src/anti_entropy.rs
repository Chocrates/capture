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

use brain_core::envelope::Envelope;

use crate::message::{Message, VersionVector};
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

/// Whether two replicas have anything to say to each other.
///
/// Both directions, because a session is opened by one side and the other may
/// be the one that is behind. A device that only pushed would never receive.
pub fn in_agreement(mine: &VersionVector, theirs: &VersionVector) -> bool {
    wanted(mine, theirs).is_empty() && wanted(theirs, mine).is_empty()
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
/// implementation in the tests rather than a direct call into `brain-store`.
pub trait Archive {
    /// One sequence number per origin, gap-free.
    fn heads(&self) -> VersionVector;

    /// Every event this origin authored in the range, in sequence order.
    fn range(&self, dev: &str, from: u64, to: u64) -> Vec<Envelope>;

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
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Exchanged {
    /// Events taken in.
    pub received: usize,
    /// Events handed over.
    pub sent: usize,
    /// Events that arrived and were refused.
    pub refused: usize,
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
    send(
        session,
        &Message::Hello {
            v: crate::message::PROTOCOL_VERSION,
            vv: mine.clone(),
            partial: false,
        },
    )?;

    let theirs = match decode(&session.recv_blocking()?) {
        Some(Message::Hello { vv, .. }) => vv,
        // Anything else here is a peer speaking a protocol this build does not
        // know. There is nothing to negotiate down to, so the round is simply
        // over.
        _ => return Err(TransportError::Malformed),
    };

    for ask in wanted(&mine, &theirs) {
        send(session, &ask)?;
    }
    send(session, &Message::Asked)?;

    let mut out = Exchanged::default();
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

fn send<T: Transport>(s: &mut Session<T>, m: &Message) -> Result<(), TransportError> {
    let mut b = Vec::new();
    minicbor::encode(m, &mut b).expect("encoding into a Vec is infallible");
    s.send(&b)
}

fn decode(b: &[u8]) -> Option<Message> {
    minicbor::decode(b).ok()
}
