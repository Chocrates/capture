//! Holding a store open for writing, and authoring into it.
//!
//! # One place where impurity enters
//!
//! Below this, nothing reads a clock or draws entropy: `brain-core` cannot name
//! either, and `brain-store` takes them as arguments. Here is where the values
//! are actually produced — the wall clock, the capture nonce, the time zone —
//! and there is exactly one function that does it, so "what did this event
//! observe about the world" has a single answer.
//!
//! # The sequence number comes from the log, not the watermark
//!
//! The watermark records how far this device has written, and is deliberately
//! written *after* the event commits. So it can lag: a crash between the two
//! leaves it one behind. Reading it to choose the next sequence number would
//! then reuse a number that is already taken.
//!
//! The log knows. `heads` is updated inside the same transaction as the row, so
//! it is never ahead of what exists and never behind it. The watermark's job is
//! to detect a *restore* — a store that has moved backwards relative to what
//! this device remembers writing — and that job needs it to be allowed to lag.

use brain_core::{
    body::{Ambient, CaptureBody},
    envelope::Envelope,
    event::{ENVELOPE_VERSION, Header, Kind},
    hlc::HlcState,
    id::BodyHash,
};
use brain_store::{
    Error, Identity, Result, StorePaths, entropy, incremental,
    lock::{self, Writer},
    log::Log,
    ops::Ops,
    projection::{self, Store},
    store,
};
use ed25519_dalek::Signer;

/// An open store this process is entitled to write to.
pub struct Session {
    pub paths: StorePaths,
    pub identity: Identity,
    pub log: Log,
    pub state: Store,
    clock: HlcState,
    /// Held for as long as the session is. Dropping it releases the store.
    _writer: Writer,
}

impl Session {
    /// Open a store for writing.
    ///
    /// Fails rather than waiting when another process holds the store. In a
    /// finished system a non-writer hands its work to the writer over a socket;
    /// until that exists, saying plainly who has it is better than blocking on
    /// something that may be a long-running overlay rather than a passing
    /// command.
    pub fn open(paths: StorePaths) -> Result<Session> {
        let identity = store::open(&paths)?;
        let lock_path = paths.root.join("writer.lock");
        let Some(writer) = Writer::acquire(&lock_path, "cli")? else {
            let who = lock::holder(&lock_path).unwrap_or_else(|| "another process".into());
            return Err(Error::Usage(format!("the store is being written by {who}")));
        };

        let log = Log::open(&paths.events)?;
        let generation = Ops::open(&paths.ops)?.generation()?;
        let state = Store::open(&projection::generation_path(&paths.state, generation))?;

        // Resume the clock. Not doing so is how a device issues a timestamp it
        // has already used: a fresh clock starts at zero, and the first event
        // after a restart would sort before everything written before it.
        let w = identity.watermark();
        let clock = HlcState::restore(w.hlc_wall, w.hlc_counter);

        Ok(Session {
            paths,
            identity,
            log,
            state,
            clock,
            _writer: writer,
        })
    }

    /// Store a capture, and return the note it became.
    pub fn capture(&mut self, text: &str) -> Result<Captured> {
        let nonce = entropy::nonce()?;
        let body = CaptureBody {
            nonce: nonce.0,
            text: text.to_string(),
            // Recorded now because it cannot be recovered later, and day
            // grouping depends on it: a note captured at 23:40 in one zone must
            // not move to a different day when read in another.
            tz: Some(local_zone()),
            ambient: Ambient::default(),
        };
        let e = self.author(Kind::CAPTURE, None, None, encode(&body))?;
        let id = e.header.id();

        self.commit(&[e])?;

        let slug = self
            .state
            .slug_of(&id)?
            .ok_or_else(|| Error::Integrity("a stored capture produced no note".into()))?;
        Ok(Captured { slug })
    }

    /// Author an event under this device's origin.
    fn author(
        &mut self,
        kind: Kind,
        tgt: Option<brain_core::EventId>,
        prev: Option<brain_core::EventId>,
        body: Vec<u8>,
    ) -> Result<Envelope> {
        let dev = self.identity.dev();
        let seq = self.next_seq()?;
        let hlc = self.clock.issue(now_ms(), dev);

        let header = Header {
            v: ENVELOPE_VERSION,
            dev,
            seq,
            hlc,
            kind,
            tgt,
            prev,
            occ: None,
            chain: self.chain_tip()?,
            bhash: BodyHash::of(&body),
        };
        let sig = self.identity.signing().sign(&header.id().0).to_bytes();
        Ok(Envelope {
            header,
            body: Some(body),
            sig,
        })
    }

    /// Append events, update the projection, then record progress.
    ///
    /// The order is the durability guarantee. The log commits first because it
    /// is the only thing that cannot be reconstructed; the projection is
    /// rebuilt from it for free, and the watermark is allowed to lag.
    fn commit(&mut self, events: &[Envelope]) -> Result<()> {
        for e in events {
            self.log.append(e)?;
        }

        let all = self.log.all()?;
        incremental::apply(&mut self.state, &all, events)?;

        if let Some(last) = events.last() {
            let (wall, counter) = self.clock.persist();
            self.identity
                .advance(&self.paths, last.header.seq, wall, counter)?;
        }
        Ok(())
    }

    fn next_seq(&self) -> Result<u64> {
        Ok(self
            .log
            .heads()?
            .get(&dev_hex(&self.identity.dev()))
            .copied()
            .unwrap_or(0)
            + 1)
    }

    fn chain_tip(&self) -> Result<Option<brain_core::EventId>> {
        let dev = self.identity.dev();
        Ok(self
            .log
            .all()?
            .into_iter()
            .filter(|e| e.header.dev == dev)
            .max_by_key(|e| e.header.seq)
            .map(|e| e.header.id()))
    }
}

/// What a capture turned into.
pub struct Captured {
    /// The note's permanent name.
    pub slug: String,
}

/// Milliseconds since the epoch.
///
/// The single wall-clock read in the writing path. Everything below takes the
/// value as an argument, which is what makes the fold reproducible: it cannot
/// consult a clock even by accident, because it cannot name one.
fn now_ms() -> u64 {
    jiff::Timestamp::now().as_millisecond().max(0) as u64
}

/// The IANA zone this machine is set to, or `UTC` if it cannot be determined.
///
/// Falling back rather than failing: a capture must not be refused because the
/// system has no zone configured. A wrong zone misfiles a note by a day at
/// worst; a refused capture loses it.
fn local_zone() -> String {
    jiff::tz::TimeZone::system()
        .iana_name()
        .unwrap_or("UTC")
        .to_string()
}

fn encode<T: minicbor::Encode<()>>(v: &T) -> Vec<u8> {
    let mut b = Vec::new();
    minicbor::encode(v, &mut b).expect("encoding into Vec is infallible");
    b
}

fn dev_hex(d: &brain_core::Dev) -> String {
    d.0.iter().map(|b| format!("{b:02x}")).collect()
}
