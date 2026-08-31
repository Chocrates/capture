//! Handing an event straight to whoever is listening.
//!
//! Two rules, and the difference between them is the whole of this module.
//!
//! For an ordinary event, push is fire-and-forget: no acknowledgement, no
//! retry, no queue. If the peer is asleep, off the network, or the socket is
//! broken, the event is simply not delivered now and anti-entropy delivers it
//! later. Push carries no correctness burden at all, which is exactly what
//! lets it be this dumb and this fast — a capture appears on the other device
//! in well under a LAN round trip, with no retry logic anywhere.
//!
//! For a `redact`, latency *is* the guarantee. The window between ordering a
//! destruction and its taking effect is the window in which the content can
//! still be read, which is the thing the feature exists to close. So a
//! redaction is pushed to every known peer rather than every open session,
//! unconnected peers are dialled, and it is retried until each one
//! acknowledges.

use alloc::vec::Vec;

use brain_core::envelope::Envelope;

use crate::message::Message;
use crate::session::Session;
use crate::transport::{Transport, TransportError};

/// Hand an event to an open session, and do not care whether it arrives.
///
/// A failure is not an error and is not reported as one. Anything that treated
/// it as one would be a thing that depends on delivery having happened, which
/// is a bug that only appears when the network is bad — the condition under
/// which it is hardest to notice and hardest to reproduce.
pub fn push<T: Transport>(session: &mut Session<T>, e: &Envelope) -> Delivered {
    match send(session, &Message::Event(e.clone())) {
        Ok(()) => Delivered::Yes,
        Err(_) => Delivered::No,
    }
}

/// Whether a push reached the wire. Not whether it reached the peer — nothing
/// here can know that, and pretending otherwise is the failure this type
/// exists to make un-writable.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delivered {
    Yes,
    No,
}

/// Hand a redaction over and wait to be told it was folded.
///
/// Unlike [`push`], this reports failure, because the caller has to keep
/// trying. The acknowledgement is what lets "gone everywhere" be an observed
/// state rather than an assumption.
pub fn push_redact<T: Transport>(
    session: &mut Session<T>,
    e: &Envelope,
) -> Result<bool, TransportError> {
    send(session, &Message::Redact(e.clone()))?;

    // Everything else on the wire is ignored while waiting — a peer may be
    // mid-exchange, and its events are not the answer to this question.
    // Anti-entropy will collect them.
    loop {
        let frame = session.recv_blocking()?;
        if let Ok(Message::RedactAck { id }) = minicbor::decode::<Message>(&frame)
            && id == e.header.id()
        {
            return Ok(true);
        }
    }
}

/// Answer a redaction that arrived: fold it, then say so.
///
/// The acknowledgement is sent *after* the store has taken it, never before.
/// Acknowledging on receipt would report a destruction that has not happened,
/// which is the one lie this protocol must not tell.
pub fn ack<T: Transport>(
    session: &mut Session<T>,
    e: &Envelope,
    took: bool,
) -> Result<(), TransportError> {
    if !took {
        return Ok(());
    }
    send(session, &Message::RedactAck { id: e.header.id() })
}

fn send<T: Transport>(s: &mut Session<T>, m: &Message) -> Result<(), TransportError> {
    let mut b = Vec::new();
    minicbor::encode(m, &mut b).expect("encoding into a Vec is infallible");
    s.send(&b)
}
