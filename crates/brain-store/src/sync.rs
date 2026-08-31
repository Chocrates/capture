//! Where the wire meets the store.
//!
//! `brain-sync` owns the protocol and knows nothing about SQLite, folding, or
//! the projection. This is the one place the two are joined, and it is
//! deliberately thin: everything arriving from a peer goes through
//! [`crate::gate::Gate`], the same door a local capture uses.
//!
//! One door matters more than it looks. A second ingest path is a second set
//! of rules about signatures, body hashes, sequence numbers and quarantine —
//! and the second set is the one that is subtly wrong, because it is the one
//! nobody exercises until a peer sends something strange.

use brain_core::envelope::Envelope;
use brain_sync::anti_entropy::Archive;
use brain_sync::message::{Seal, VersionVector};

use crate::gate::{Gate, Outcome};

/// This device's archive, as the protocol sees it.
pub struct StoreArchive<'a, 'g> {
    gate: &'g mut Gate<'a>,
    /// Wall-clock milliseconds, supplied rather than read, so a sync is
    /// testable without waiting for time to pass.
    now: u64,
}

impl<'a, 'g> StoreArchive<'a, 'g> {
    pub fn new(gate: &'g mut Gate<'a>, now: u64) -> Self {
        Self { gate, now }
    }
}

impl Archive for StoreArchive<'_, '_> {
    fn heads(&self) -> VersionVector {
        // Straight from the log, including retired origins. Leaving a sealed
        // origin out is the completeness-oracle failure: a peer would ask
        // nothing about it and report agreement having received none of it.
        self.gate.log().heads().unwrap_or_default()
    }

    fn sealed(&self) -> Vec<Seal> {
        // Nothing is sealed yet. Sealing needs a quorum among reachable
        // members, which needs the daemon; until then every origin is treated
        // as live, which is correct and merely does more work.
        Vec::new()
    }

    fn range(&self, dev: &str, from: u64, to: u64) -> Vec<Envelope> {
        self.gate.log().range(dev, from, to).unwrap_or_default()
    }

    fn ingest(&mut self, e: Envelope) -> bool {
        // Relaying confers no authority. The event is verified against the key
        // the *roster* names for its claimed origin, never against whoever
        // handed it over — transport security proves "the desktop sent this",
        // and only a per-event signature proves "the phone wrote it".
        match self.gate.offer(&e, self.now) {
            Ok(Outcome::Took(_)) => true,
            // Set aside rather than taken: authentic, stored at its position,
            // relayed onward, excluded from the fold until whatever it is
            // waiting for arrives. Reported as taken because it *was* taken —
            // the alternative is a peer re-sending it for ever.
            Ok(Outcome::SetAside(_)) => true,
            Ok(_) | Err(_) => false,
        }
    }
}
