//! Device identity: the keypair, the incarnation, and the write watermark.
//!
//! # Two files, not one
//!
//! The task list describes a single "identity record" holding the keypair, the
//! incarnation, and the highest sequence number written. That is one file doing
//! two jobs with opposite access patterns:
//!
//! - The keypair and incarnation are written **once** and are secret.
//! - The watermark is rewritten **on every append** and is not.
//!
//! Keeping them together would mean rewriting a file containing a private key
//! on the capture hot path, several times a minute, forever. Every rewrite is a
//! window in which a crash can leave the key truncated — and losing the key
//! means losing the ability to author under this origin at all.
//!
//! So: `config/identity.key` (mode 0600, written once) and `config/watermark`
//! (rewritten freely, contains nothing secret).
//!
//! # Why the watermark exists at all
//!
//! Startup has to distinguish "the store fell behind the identity" — meaning
//! the event log was restored, replaced, or lost, and this device must mint a
//! new origin before writing anything — from "the identity fell behind the
//! store", which is the ordinary result of a crash between committing an event
//! and updating the watermark.
//!
//! Those two look identical without a recorded high-water mark, and treating
//! them the same is a real bug in either direction: mint an origin after every
//! crash, or reuse sequence numbers other devices already hold.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use brain_core::{Dev, Incarnation};
use ed25519_dalek::{SigningKey, VerifyingKey};
use minicbor::{Decode, Encode};

use crate::{Error, Result, StorePaths};

/// Format version for the identity record. Bumped only on a deliberate change.
pub const IDENTITY_VERSION: u16 = 1;

/// The parts of identity that never change once created.
///
/// Written once, at mode 0600. If this file is lost the device cannot author
/// under its existing origin, so it is the one thing here worth backing up
/// separately.
#[derive(Clone, Encode, Decode)]
pub struct IdentityFile {
    #[n(0)]
    version: u16,
    /// Ed25519 signing key. Secret.
    #[cbor(n(1), with = "minicbor::bytes")]
    secret: [u8; 32],
    /// Current incarnation. Re-minted on restore, never incremented.
    #[cbor(n(2), with = "minicbor::bytes")]
    incarnation: [u8; 16],
    /// X25519 secret, for answering a Noise handshake (§5.2). Secret.
    ///
    /// Optional because identities written before sync existed do not have one.
    /// Such a device generates one the next time it is loaded and keeps it; what
    /// it cannot do by itself is *publish* the public half, which is a
    /// membership event and therefore a deliberate act.
    #[cbor(n(3), with = "minicbor::bytes")]
    noise: Option<[u8; 32]>,
}

/// The frequently-rewritten half: how far this device has written.
///
/// Contains nothing secret, so it can be rewritten on the hot path without
/// putting the key at risk.
#[derive(Clone, Copy, Encode, Decode, Debug, PartialEq, Eq)]
pub struct Watermark {
    #[n(0)]
    pub version: u16,
    /// Highest sequence number this device has committed under the current
    /// origin. Zero means nothing has been written yet.
    #[n(1)]
    pub highest_seq: u64,
    /// Last emitted clock reading — physical milliseconds.
    #[n(2)]
    pub hlc_wall: u64,
    /// Last emitted clock counter.
    #[n(3)]
    pub hlc_counter: u32,
}

impl Watermark {
    fn new() -> Self {
        Watermark {
            version: IDENTITY_VERSION,
            highest_seq: 0,
            hlc_wall: 0,
            hlc_counter: 0,
        }
    }
}

/// A loaded device identity.
pub struct Identity {
    signing: SigningKey,
    noise: [u8; 32],
    incarnation: Incarnation,
    dev: Dev,
    watermark: Watermark,
}

impl Identity {
    /// Create a brand-new identity: fresh keypair, fresh incarnation.
    ///
    /// The incarnation is **random, not a counter**. A counter has to be read
    /// from somewhere, and the thing you would read it from is exactly what a
    /// restore has just lost — so restoring twice, or restoring a stale copy,
    /// would mint the same origin twice and collide at every sequence position.
    pub fn create(paths: &StorePaths) -> Result<Self> {
        let mut secret = [0u8; 32];
        crate::entropy::fill(&mut secret)?;
        let mut inc = [0u8; 16];
        crate::entropy::fill(&mut inc)?;
        let mut noise = [0u8; 32];
        crate::entropy::fill(&mut noise)?;

        let file = IdentityFile {
            version: IDENTITY_VERSION,
            secret,
            incarnation: inc,
            noise: Some(noise),
        };
        write_secret(&paths.identity, &to_cbor(&file))?;

        let wm = Watermark::new();
        write_plain(&watermark_path(paths), &to_cbor(&wm))?;

        Ok(Self::assemble(file, wm))
    }

    /// Load an existing identity.
    pub fn load(paths: &StorePaths) -> Result<Self> {
        let raw =
            fs::read(&paths.identity).map_err(|_| Error::StoreMissing(paths.identity.clone()))?;
        let mut file: IdentityFile = minicbor::decode(&raw)
            .map_err(|e| Error::Integrity(format!("identity record unreadable: {e}")))?;

        // An identity from before sync existed has no Noise key. It gets one
        // here and keeps it. Publishing the public half is a separate and
        // deliberate act — it is a membership event — so this only makes the
        // device *able* to be paired, it does not pair it.
        if file.noise.is_none() {
            let mut n = [0u8; 32];
            crate::entropy::fill(&mut n)?;
            file.noise = Some(n);
            write_secret(&paths.identity, &to_cbor(&file))?;
        }
        if file.version != IDENTITY_VERSION {
            return Err(Error::Integrity(format!(
                "identity record version {}, expected {IDENTITY_VERSION}",
                file.version
            )));
        }

        // A missing watermark is recoverable — it means nothing was written, or
        // the file was lost. Treating it as zero makes startup detection report
        // "store is ahead", which is the safe direction (§ startup states).
        let wm = match fs::read(watermark_path(paths)) {
            Ok(b) => minicbor::decode(&b).unwrap_or_else(|_| Watermark::new()),
            Err(_) => Watermark::new(),
        };

        Ok(Self::assemble(file, wm))
    }

    fn assemble(file: IdentityFile, watermark: Watermark) -> Self {
        let signing = SigningKey::from_bytes(&file.secret);
        let incarnation = Incarnation(file.incarnation);
        let dev = Dev::derive(&signing.verifying_key().to_bytes(), &incarnation);
        Identity {
            signing,
            // Filled by `load` before it gets here. An identity written before
            // sync existed has none on disk, and gains one the first time it is
            // opened rather than the first time somebody tries to pair — a key
            // that appears mid-gesture is a key that can fail mid-gesture.
            noise: file.noise.unwrap_or([0u8; 32]),
            incarnation,
            dev,
            watermark,
        }
    }

    /// Mint a new incarnation, keeping the existing keypair.
    ///
    /// This is what a restore does. The device stays the same roster member —
    /// membership is keyed by public key — but becomes a **new origin**, so it
    /// cannot reuse sequence numbers that peers already hold under the old one.
    ///
    /// The watermark resets to zero because the new origin has written nothing.
    pub fn reincarnate(&mut self, paths: &StorePaths) -> Result<()> {
        let mut inc = [0u8; 16];
        crate::entropy::fill(&mut inc)?;

        let file = IdentityFile {
            version: IDENTITY_VERSION,
            secret: self.signing.to_bytes(),
            incarnation: inc,
            // Kept across a reincarnation. A restore makes a new *origin*, not
            // a new device — roster membership is keyed by the device key, and
            // the Noise key is the same kind of fact about the same device.
            noise: Some(self.noise),
        };
        write_secret(&paths.identity, &to_cbor(&file))?;

        self.incarnation = Incarnation(inc);
        self.dev = Dev::derive(&self.public().to_bytes(), &self.incarnation);
        self.watermark = Watermark::new();
        write_plain(&watermark_path(paths), &to_cbor(&self.watermark))?;
        Ok(())
    }

    /// This device's current origin.
    pub fn dev(&self) -> Dev {
        self.dev
    }

    /// The public half of the device key. Roster membership is keyed by this,
    /// which is why a restore does not remove the device from the roster.
    pub fn public(&self) -> VerifyingKey {
        self.signing.verifying_key()
    }

    /// The public half of this device's Noise key, which is what a peer needs
    /// to open a session with it.
    pub fn noise_public(&self) -> [u8; 32] {
        x25519_dalek::PublicKey::from(&x25519_dalek::StaticSecret::from(self.noise)).to_bytes()
    }

    /// The secret half. Handed to the handshake and nowhere else.
    pub fn noise_secret(&self) -> [u8; 32] {
        self.noise
    }

    /// Signing key, for authoring events.
    pub fn signing(&self) -> &SigningKey {
        &self.signing
    }

    /// Current incarnation.
    pub fn incarnation(&self) -> Incarnation {
        self.incarnation
    }

    /// Current watermark.
    pub fn watermark(&self) -> Watermark {
        self.watermark
    }

    /// Record progress after committing an event.
    ///
    /// Deliberately allowed to lag the log: this file is written after the
    /// event commits, so a crash in between leaves the watermark behind, which
    /// startup treats as ordinary (§ startup states) rather than as a restore.
    pub fn advance(
        &mut self,
        paths: &StorePaths,
        seq: u64,
        hlc_wall: u64,
        hlc_counter: u32,
    ) -> Result<()> {
        self.watermark.highest_seq = self.watermark.highest_seq.max(seq);
        self.watermark.hlc_wall = hlc_wall;
        self.watermark.hlc_counter = hlc_counter;
        write_plain(&watermark_path(paths), &to_cbor(&self.watermark))
    }
}

fn watermark_path(paths: &StorePaths) -> std::path::PathBuf {
    paths
        .identity
        .parent()
        .unwrap_or(&paths.root)
        .join("watermark")
}

fn to_cbor<T: Encode<()>>(v: &T) -> Vec<u8> {
    let mut b = Vec::new();
    minicbor::encode(v, &mut b).expect("encoding into Vec is infallible");
    b
}

/// Write a file that must not be world-readable, creating it 0600 from the
/// start rather than fixing the mode afterwards — the gap between create and
/// chmod is a window where the key is readable.
fn write_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

fn write_plain(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut f = fs::File::create(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> StorePaths {
        let dir = std::env::temp_dir().join(format!("brain-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        StorePaths::under(dir)
    }

    #[test]
    fn create_then_load_round_trips() {
        let p = tmp("ident-roundtrip");
        let a = Identity::create(&p).unwrap();
        let b = Identity::load(&p).unwrap();
        assert_eq!(a.dev(), b.dev());
        assert_eq!(a.public().to_bytes(), b.public().to_bytes());
        fs::remove_dir_all(&p.root).ok();
    }

    #[test]
    fn key_file_is_not_world_readable() {
        let p = tmp("ident-mode");
        Identity::create(&p).unwrap();
        let mode = fs::metadata(&p.identity).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "private key must not be readable by others");
        fs::remove_dir_all(&p.root).ok();
    }

    #[test]
    fn reincarnation_changes_origin_but_keeps_the_key() {
        let p = tmp("ident-reinc");
        let mut id = Identity::create(&p).unwrap();
        let before_dev = id.dev();
        let before_key = id.public().to_bytes();

        id.reincarnate(&p).unwrap();

        assert_ne!(
            before_dev,
            id.dev(),
            "a restored device must be a new origin"
        );
        assert_eq!(
            before_key,
            id.public().to_bytes(),
            "the key must survive, or the device leaves the roster"
        );
        fs::remove_dir_all(&p.root).ok();
    }

    #[test]
    fn reincarnation_is_not_reproducible_from_a_stale_copy() {
        // The property a counter would not have: two restores never collide.
        let p1 = tmp("ident-r1");
        let p2 = tmp("ident-r2");
        let mut a = Identity::create(&p1).unwrap();
        let mut b = Identity::create(&p2).unwrap();
        a.reincarnate(&p1).unwrap();
        b.reincarnate(&p2).unwrap();
        assert_ne!(a.incarnation().0, b.incarnation().0);
        fs::remove_dir_all(&p1.root).ok();
        fs::remove_dir_all(&p2.root).ok();
    }

    #[test]
    fn reincarnation_resets_the_watermark() {
        let p = tmp("ident-wm-reset");
        let mut id = Identity::create(&p).unwrap();
        id.advance(&p, 41822, 1000, 3).unwrap();
        assert_eq!(id.watermark().highest_seq, 41822);

        id.reincarnate(&p).unwrap();
        assert_eq!(
            id.watermark().highest_seq,
            0,
            "a new origin has written nothing"
        );
        fs::remove_dir_all(&p.root).ok();
    }

    #[test]
    fn watermark_survives_reload() {
        let p = tmp("ident-wm-reload");
        let mut id = Identity::create(&p).unwrap();
        id.advance(&p, 7, 12_345, 2).unwrap();
        let again = Identity::load(&p).unwrap();
        assert_eq!(again.watermark().highest_seq, 7);
        assert_eq!(again.watermark().hlc_wall, 12_345);
        assert_eq!(again.watermark().hlc_counter, 2);
        fs::remove_dir_all(&p.root).ok();
    }

    #[test]
    fn watermark_never_moves_backwards() {
        let p = tmp("ident-wm-mono");
        let mut id = Identity::create(&p).unwrap();
        id.advance(&p, 10, 1, 0).unwrap();
        id.advance(&p, 4, 2, 0).unwrap();
        assert_eq!(id.watermark().highest_seq, 10);
        fs::remove_dir_all(&p.root).ok();
    }

    #[test]
    fn a_missing_watermark_reads_as_zero() {
        // The safe direction: startup then sees "store ahead of identity",
        // which is ordinary, rather than "identity ahead of store", which
        // triggers a new incarnation.
        let p = tmp("ident-wm-missing");
        let mut id = Identity::create(&p).unwrap();
        id.advance(&p, 99, 1, 0).unwrap();
        fs::remove_file(watermark_path(&p)).unwrap();
        let again = Identity::load(&p).unwrap();
        assert_eq!(again.watermark().highest_seq, 0);
        fs::remove_dir_all(&p.root).ok();
    }
}
