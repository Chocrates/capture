//! Moving bytes that are too big to live in the log (§5.6).
//!
//! # Inside the session, like everything else
//!
//! These messages travel in the Noise session the events came through. A
//! separate byte-serving path is the easy mistake the design names by name: it
//! would put a recording in the clear on the local network while the note
//! describing it was encrypted, which is worse than not having the feature at
//! all — the note says "voice note about the thing" and the recording says
//! everything.
//!
//! # One chunk per round trip
//!
//! Not because it is fast — it is not, particularly — but because the
//! alternative is a sender queueing forty megabytes into memory ahead of a
//! receiver, on the smallest device in the roster. Asking for the next chunk
//! only once the last one has landed bounds the memory at one chunk and makes
//! resumption fall out for free: the offset asked for *is* the progress.
//!
//! # One direction at a time
//!
//! The event round is symmetric because both sides can stream events past each
//! other without either having to hold much. Bytes are different, so this runs
//! as two phases with a clear handoff: whoever dialled fetches what it wants,
//! then the other does. Interleaving them would mean each side answering
//! requests while awaiting its own, and a stall in one direction would look
//! exactly like a stall in the other.

use crate::message::Message;
use crate::session::Session;
use crate::transport::{Transport, TransportError};

/// The size of one chunk.
///
/// 32 KiB, chosen so a chunk plus its framing fits inside a single Noise
/// transport message with room to spare. Checked by a golden test rather than
/// trusted: an earlier draft of the design said 65536, which does not fit at
/// all, and nothing would have found out until somebody first moved a
/// recording.
pub const CHUNK: usize = 32 * 1024;

/// The largest blob this will accept in one transfer.
///
/// A ceiling, because a peer states the total and a receiver believes it
/// enough to keep reading. Without one, a peer that said "this is eighteen
/// exabytes" would have the other filling its disk in good faith.
pub const MAX_BLOB: u64 = 512 * 1024 * 1024;

/// What a replica needs to hold up its end of a blob transfer.
///
/// Separate from the event-round trait because a replica can be perfectly
/// correct and hold no bytes at all: a phone that has evicted everything still
/// syncs every event, and §5.6 makes that the recommended state for a
/// constrained device rather than a degraded one.
pub trait Blobs {
    /// Names this replica refers to and does not hold.
    fn wanted(&self) -> Vec<String> {
        Vec::new()
    }

    /// How much of one is already here, so a transfer resumes.
    fn have_bytes(&self, _hash: &str) -> u64 {
        0
    }

    /// A chunk, and the total size, if this replica holds the bytes.
    fn chunk(&self, _hash: &str, _offset: u64) -> Option<(u64, Vec<u8>)> {
        None
    }

    /// Take a chunk that arrived. Returns whether the blob is now complete.
    fn store(&mut self, _hash: &str, _offset: u64, _total: u64, _data: &[u8]) -> bool {
        false
    }
}

/// What a transfer phase managed.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Moved {
    /// Blobs fetched to completion.
    pub fetched: usize,
    /// Bytes received, across all of them.
    pub bytes: u64,
    /// Blobs the peer did not have. Not a failure: an evicted blob is the
    /// ordinary state of a constrained device, and the answer is to ask
    /// somebody else.
    pub missing: usize,
    /// Whether the phase stopped on its budget rather than on finishing.
    ///
    /// Not a failure either, and worth distinguishing: a round that moved what
    /// it could and will continue next time is a different report from one that
    /// found nothing left to move.
    pub ran_out_of_time: bool,
}

/// Fetch what this replica wants, one blob at a time.
///
/// The peer must be running [`serve`] for the same stretch.
///
/// # The budget
///
/// `spent` says whether the round has used the time it was given. It is asked
/// between chunks rather than between blobs, because a single large blob over a
/// slow link is the case that matters: one ten-minute voice note was enough to
/// occupy a phone's whole sync window, and the window is not negotiable — it is
/// a `WorkManager` job that the platform destroys mid-sentence when the budget
/// ends.
///
/// Stopping costs nothing, which is what makes a deadline the right shape here.
/// A partial blob is a partial file on disk and `have_bytes` is where the next
/// round resumes from, so a big attachment crosses several rounds and arrives,
/// instead of one round restarting it for ever and finishing none of them.
///
/// A predicate rather than an instant, because this crate does not read clocks
/// (DESIGN.md D12) — the caller owns the clock and the deadline computed from
/// it, exactly as `round::bounded` does for the rest of the round.
pub fn fetch<T: Transport, B: Blobs>(
    session: &mut Session<T>,
    blobs: &mut B,
    spent: &dyn Fn() -> bool,
) -> Result<Moved, TransportError> {
    let mut moved = Moved::default();

    for hash in blobs.wanted() {
        if spent() {
            moved.ran_out_of_time = true;
            break;
        }
        let mut offset = blobs.have_bytes(&hash);

        loop {
            send(
                session,
                &Message::Want {
                    hash: hash.clone(),
                    have_bytes: offset,
                },
            )?;

            match decode(&session.recv_blocking()?) {
                Some(Message::Blob {
                    hash: said,
                    offset: at,
                    total,
                    data,
                }) => {
                    // The peer answering about a different blob, or from a
                    // different place than was asked for, is a peer whose
                    // answers cannot be assembled into anything. Ending the
                    // round is the only safe response: writing it anyway
                    // would produce a file whose name is a lie.
                    if said != hash || at != offset {
                        return Err(TransportError::Malformed);
                    }
                    if total > MAX_BLOB {
                        return Err(TransportError::Malformed);
                    }
                    moved.bytes += data.len() as u64;
                    offset += data.len() as u64;

                    let done = blobs.store(&hash, at, total, &data);
                    if done {
                        moved.fetched += 1;
                        break;
                    }
                    // Between chunks, so a single large blob cannot hold the
                    // whole window. What has arrived is already on disk and the
                    // next round resumes from it.
                    if spent() {
                        moved.ran_out_of_time = true;
                        break;
                    }
                    // A peer that keeps answering with nothing, forever, is
                    // not making progress and is not going to.
                    if data.is_empty() {
                        return Err(TransportError::Malformed);
                    }
                }
                Some(Message::NoBlob { .. }) => {
                    moved.missing += 1;
                    break;
                }
                // Anything else means the peer is not in this phase, which
                // there is no recovering from mid-transfer.
                _ => return Err(TransportError::Malformed),
            }
        }
    }

    // Said, always, and on every path out. The peer is inside `serve` and
    // leaves it only when told; a fetch that stopped early without saying so
    // would leave it blocked on a message that is never coming, and the round
    // would end on a timeout at both ends rather than on a decision at one.
    send(session, &Message::Done)?;
    Ok(moved)
}

/// Answer whatever the peer asks for, until it says it is finished.
pub fn serve<T: Transport, B: Blobs>(
    session: &mut Session<T>,
    blobs: &B,
) -> Result<usize, TransportError> {
    let mut served = 0;

    loop {
        match decode(&session.recv_blocking()?) {
            Some(Message::Want { hash, have_bytes }) => match blobs.chunk(&hash, have_bytes) {
                Some((total, data)) => {
                    served += 1;
                    send(
                        session,
                        &Message::Blob {
                            hash,
                            offset: have_bytes,
                            total,
                            data,
                        },
                    )?;
                }
                // Said rather than left to a timeout. A device that evicted
                // this is in the ordinary case, and the asker should go
                // elsewhere at once — eviction is only cheap if a miss is
                // fast.
                None => send(session, &Message::NoBlob { hash })?,
            },
            Some(Message::Done) => return Ok(served),
            _ => return Err(TransportError::Malformed),
        }
    }
}

fn send<T: Transport>(session: &mut Session<T>, m: &Message) -> Result<(), TransportError> {
    let mut b = Vec::new();
    minicbor::encode(m, &mut b).map_err(|_| TransportError::Malformed)?;
    session.send(&b)
}

fn decode(bytes: &[u8]) -> Option<Message> {
    minicbor::decode(bytes).ok()
}
