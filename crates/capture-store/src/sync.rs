//! Where the wire meets the store.
//!
//! `capture-sync` owns the protocol and knows nothing about SQLite, folding, or
//! the projection. This is the one place the two are joined, and it is
//! deliberately thin: everything arriving from a peer goes through
//! [`crate::gate::Gate`], the same door a local capture uses.
//!
//! One door matters more than it looks. A second ingest path is a second set
//! of rules about signatures, body hashes, sequence numbers and quarantine —
//! and the second set is the one that is subtly wrong, because it is the one
//! nobody exercises until a peer sends something strange.

use capture_core::envelope::Envelope;
use capture_sync::anti_entropy::Archive;
use capture_sync::message::{Seal, VersionVector};

use crate::gate::{Gate, Outcome};
use crate::ops::Ops;

/// This device's archive, as the protocol sees it.
pub struct StoreArchive<'a, 'g> {
    gate: &'g mut Gate<'a>,
    /// Where the record of what a redaction still owes lives.
    ///
    /// Separate from the log on purpose: what has been *delivered* is local
    /// knowledge, not a fact about the archive, and replicating it would make
    /// every device's view of it disagree.
    ops: &'g Ops,
    /// Wall-clock milliseconds, supplied rather than read, so a sync is
    /// testable without waiting for time to pass.
    now: u64,
    /// What was taken this round, waiting to be folded.
    ///
    /// Accumulated rather than folded per event. A fold re-computes every note
    /// an event touches, and doing that once per arriving event would make
    /// catching up after a week off quadratic in the number of edits to the
    /// same note — the same lesson `append_all` learned when writing five
    /// review events one at a time cost a third of a second.
    taken: Vec<Envelope>,
}

impl<'a, 'g> StoreArchive<'a, 'g> {
    pub fn new(gate: &'g mut Gate<'a>, ops: &'g Ops, now: u64) -> Self {
        Self {
            gate,
            ops,
            now,
            taken: Vec::new(),
        }
    }

    /// Which member a Noise key belongs to.
    ///
    /// The handshake establishes an X25519 key; everything else about a device
    /// — membership, who a redaction is owed to, who paired it — is keyed by
    /// its Ed25519 signing key. The two are deliberately separate (one proves
    /// authorship, the other secures a conversation), so the translation has
    /// to happen somewhere, and doing it here means no caller can forget.
    ///
    /// The first version of this compared the two directly. Nothing failed to
    /// compile, nothing errored, and every redaction simply stayed outstanding
    /// for ever — which is the safe direction, and still wrong.
    pub fn member_behind(&self, noise: &[u8; 32]) -> Option<[u8; 32]> {
        for e in self.gate.log().all().ok()? {
            if !matches!(
                e.header.kind,
                capture_core::event::Kind::DEVICE_ADD | capture_core::event::Kind::ORIGIN_CLAIM
            ) {
                continue;
            }
            let Some(body) = e.body.as_deref() else {
                continue;
            };
            let Ok(claim) = minicbor::decode::<capture_core::body::OriginClaimBody>(body) else {
                continue;
            };
            if claim.noise.as_ref() == Some(noise) {
                return Some(claim.pubkey);
            }
        }
        None
    }

    /// The highest stamp anything authentic carried in, for the session to adopt.
    pub fn highest_stamp(&self) -> Option<capture_core::Hlc> {
        self.gate.highest_stamp()
    }

    /// Fold everything this round took into the projection.
    ///
    /// Must be called after the exchange. `Gate` puts events in the log and
    /// nothing else — the log is the archive, and the projection is derived
    /// data that something has to derive. Without this a sync moves every
    /// event correctly and no surface shows any of them, which is the failure
    /// the two-device test caught.
    pub fn settle(&mut self) -> crate::Result<usize> {
        if self.taken.is_empty() {
            return Ok(0);
        }
        let taken = core::mem::take(&mut self.taken);
        self.gate.fold(&taken, self.now as i64)
    }

    /// How many events are waiting to be folded.
    pub fn pending(&self) -> usize {
        self.taken.len()
    }
}

impl Archive for StoreArchive<'_, '_> {
    fn heads(&self) -> VersionVector {
        // Straight from the log, including retired origins. Leaving a sealed
        // origin out is the completeness-oracle failure: a peer would ask
        // nothing about it and report agreement having received none of it.
        self.gate.log().heads().unwrap_or_default()
    }

    fn observes(&self) -> Vec<String> {
        // What this build, on this platform, can actually watch for (§7.5).
        // Asked of the same function every other caller asks, so a device
        // cannot advertise a cue it would refuse to create.
        [
            "time",
            "recurring",
            "device_wake",
            "location",
            "app_focus",
            "git_branch",
            "calendar_start",
        ]
        .into_iter()
        .filter(|c| crate::notifier::observable(c))
        .map(String::from)
        // And whether it can speak at all, so the default notifier skips a
        // device that would say nothing (§2.4.1).
        .chain(
            (self.ops.get("alerts.silenced").ok().flatten().as_deref() == Some("1"))
                .then(|| crate::notifier::SILENT.to_string()),
        )
        .collect()
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

    fn owed_redactions(&self, peer: Option<[u8; 32]>) -> Vec<Envelope> {
        let Some(peer) = peer.and_then(|n| self.member_behind(&n)) else {
            return Vec::new();
        };
        let Ok(outstanding) = self.ops.redactions_outstanding() else {
            return Vec::new();
        };

        outstanding
            .into_iter()
            .filter(|(_, owed)| owed.contains(&peer))
            // Owed but not held is possible — a redaction can be recorded as
            // owed on a device that has since been rebuilt from an export
            // taken before it. Nothing here can produce the bytes, so it stays
            // owed and stays reported, which is the true statement.
            .filter_map(|(id, _)| self.gate.log().by_id(&id).ok().flatten())
            .collect()
    }

    fn redaction_acked(&mut self, id: capture_core::EventId, peer: Option<[u8; 32]>) {
        if let Some(peer) = peer.and_then(|n| self.member_behind(&n)) {
            let _ = self.ops.redact_acked(&id, &peer, self.now);
        }
    }

    fn ingest(&mut self, e: Envelope) -> bool {
        // Relaying confers no authority. The event is verified against the key
        // the *roster* names for its claimed origin, never against whoever
        // handed it over — transport security proves "the desktop sent this",
        // and only a per-event signature proves "the phone wrote it".
        match self.gate.offer(&e, self.now) {
            Ok(Outcome::Took(crate::log::Ingested::Duplicate)) => true,
            Ok(Outcome::Took(_)) => {
                self.taken.push(e);
                true
            }
            // Set aside rather than taken: authentic, stored at its position,
            // relayed onward, excluded from the fold until whatever it is
            // waiting for arrives. Reported as taken because it *was* taken —
            // the alternative is a peer re-sending it for ever.
            Ok(Outcome::SetAside(_)) => true,
            Ok(_) | Err(_) => false,
        }
    }
}

/// This device's bytes, for the blob phase of a round (§5.6).
///
/// Reads the log for what is referenced rather than the projection, because
/// the projection has no blob table yet. That is a scan per round, which is
/// honest at present volumes and is the obvious thing to replace with a table
/// when it stops being — the schema may add tables, so it is a change with
/// somewhere to go rather than a wall.
pub struct StoreBlobs {
    paths: crate::StorePaths,
}

impl StoreBlobs {
    pub fn new(paths: crate::StorePaths) -> Self {
        StoreBlobs { paths }
    }

    /// Every blob the log refers to.
    fn referenced(&self) -> Vec<(String, capture_core::body::BlobRef)> {
        let Ok(log) = crate::log::Log::open_read(&self.paths.events) else {
            return Vec::new();
        };
        let Ok(events) = log.all() else {
            return Vec::new();
        };

        let mut out = Vec::new();
        for e in events {
            if e.header.kind != capture_core::event::Kind::CAPTURE {
                continue;
            }
            let Some(body) = e.body.as_deref() else {
                // Redacted. The reference is gone with the body, which is the
                // point: a destroyed note does not leave a device fetching the
                // recording it was destroyed to be rid of.
                continue;
            };
            let Ok(capture) = minicbor::decode::<capture_core::body::CaptureBody>(body) else {
                continue;
            };
            if let Some(blob) = capture.blob {
                out.push((crate::blobs::name(&blob.hash), blob));
            }
        }
        out
    }
}

impl capture_sync::blob::Blobs for StoreBlobs {
    fn wanted(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .referenced()
            .into_iter()
            .filter(|(name, _)| {
                crate::blobs::parse(name)
                    .map(|h| !crate::blobs::have(&self.paths.blobs, &h))
                    .unwrap_or(false)
            })
            .map(|(name, _)| name)
            .collect();
        names.sort();
        names.dedup();
        names
    }

    fn have_bytes(&self, hash: &str) -> u64 {
        crate::blobs::parse(hash)
            .map(|h| crate::blobs::partial_bytes(&self.paths.blobs, &h))
            .unwrap_or(0)
    }

    fn chunk(&self, hash: &str, offset: u64) -> Option<(u64, Vec<u8>)> {
        let h = crate::blobs::parse(hash)?;
        // Ranged, not whole-file. See `blobs::range`: reading and hashing the
        // entire blob once per 32 KiB chunk made serving a voice note
        // quadratic, inside the writer lock, on the phone.
        crate::blobs::range(&self.paths.blobs, &h, offset, capture_sync::blob::CHUNK).ok()?
    }

    fn store(&mut self, hash: &str, offset: u64, total: u64, data: &[u8]) -> bool {
        let Some(h) = crate::blobs::parse(hash) else {
            return false;
        };
        let done = crate::blobs::take(&self.paths.blobs, &h, offset, total, data).unwrap_or(false);
        if done {
            // Refusals are about a blob this device does not have. Kept past
            // its arrival, a later eviction would inherit them and be called
            // unavailable on its first miss.
            if let Ok(ops) = crate::ops::Ops::open(&self.paths.ops) {
                let _ = ops.blob_arrived(hash);
            }
        }
        done
    }
}
