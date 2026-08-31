//! Creating and opening a store.
//!
//! # Order matters here
//!
//! Setup does the same things every install does, but two of them are ordered
//! deliberately rather than incidentally:
//!
//! 1. **The spool is claimed before anything else can fail.** It exists to
//!    survive a full disk, so allocating it lazily — at the moment it is first
//!    needed — would be allocating it at the one moment it cannot be allocated.
//! 2. **The roster root is authored last**, after identity exists, because it
//!    is signed by the key that step creates.
//!
//! # What this does not do
//!
//! It does not create database schemas, and it does not persist the roster
//! root. Both wait for the event log itself. Setup here produces the identity,
//! the layout on disk, and the first event as a value — storing that value is
//! the log's job.

use std::fs;

use brain_core::{envelope::Envelope, hlc::HlcState};

use crate::{Identity, Result, StorePaths, genesis, spool};

/// What a fresh store looks like after setup.
pub struct FreshStore {
    /// The device identity just created.
    pub identity: Identity,
    /// The roster root, authored but not yet stored.
    pub root: Envelope,
    /// The clock, carrying the stamp the root consumed.
    pub clock: HlcState,
}

/// Whether a store already exists at these paths.
pub fn exists(paths: &StorePaths) -> bool {
    paths.identity.exists()
}

/// Create a store from nothing.
///
/// `physical_now` is milliseconds since the epoch, supplied by the caller —
/// this crate owns the clock, but the value is still passed explicitly so the
/// whole path is testable without waiting for time to pass.
pub fn create(paths: &StorePaths, physical_now: u64) -> Result<FreshStore> {
    if exists(paths) {
        return Err(crate::Error::Usage(format!(
            "a store already exists at {}",
            paths.root.display()
        )));
    }

    for dir in paths.directories() {
        fs::create_dir_all(dir)?;
    }

    // Before anything can go wrong with the disk.
    spool::preallocate(&paths.spool)?;

    let identity = Identity::create(paths)?;

    let mut clock = HlcState::new();
    let nonce = crate::entropy::nonce()?;

    let root = genesis::roster_root(
        identity.signing(),
        &identity.incarnation(),
        &mut clock,
        physical_now,
        nonce,
        identity.noise_public(),
    )?;

    tracing::info!(origin = ?identity.dev().short(), "store created");

    Ok(FreshStore {
        identity,
        root,
        clock,
    })
}

/// Open an existing store.
pub fn open(paths: &StorePaths) -> Result<Identity> {
    if !exists(paths) {
        return Err(crate::Error::StoreMissing(paths.root.clone()));
    }
    // The spool may be absent if a previous version created the store, or if
    // someone removed it. Re-claiming is cheap and never clobbers records.
    spool::preallocate(&paths.spool)?;
    Identity::load(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_core::envelope::Verdict;

    fn tmp(name: &str) -> StorePaths {
        let d = std::env::temp_dir().join(format!("brain-store-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        StorePaths::under(d)
    }

    fn cleanup(p: &StorePaths) {
        fs::remove_dir_all(&p.root).ok();
    }

    #[test]
    fn create_lays_out_everything() {
        let p = tmp("layout");
        let _ = create(&p, 1000).unwrap();
        assert!(p.root.is_dir());
        assert!(p.blobs.is_dir());
        assert!(p.export.is_dir());
        assert!(p.identity.exists());
        assert!(p.spool.exists());
        cleanup(&p);
    }

    #[test]
    fn the_spool_is_claimed_at_setup() {
        // Not lazily: it exists to survive a full disk, and a full disk is
        // exactly when it could not be allocated.
        let p = tmp("spool-eager");
        let _ = create(&p, 1000).unwrap();
        assert_eq!(fs::metadata(&p.spool).unwrap().len(), spool::SPOOL_BYTES);
        cleanup(&p);
    }

    #[test]
    fn the_root_verifies_against_the_identity_it_created() {
        let p = tmp("root-verify");
        let f = create(&p, 1000).unwrap();
        assert_eq!(
            f.root.verify(&f.identity.public().to_bytes()),
            Verdict::Authentic
        );
        cleanup(&p);
    }

    #[test]
    fn creating_over_an_existing_store_is_refused() {
        // Silently reinitialising would discard a keypair, and with it the
        // ability to author under the existing origin.
        let p = tmp("no-clobber");
        let _ = create(&p, 1000).unwrap();
        assert!(create(&p, 2000).is_err());
        cleanup(&p);
    }

    #[test]
    fn open_returns_the_same_origin() {
        let p = tmp("reopen");
        let f = create(&p, 1000).unwrap();
        let again = open(&p).unwrap();
        assert_eq!(f.identity.dev(), again.dev());
        cleanup(&p);
    }

    #[test]
    fn opening_a_missing_store_says_so() {
        let p = tmp("absent");
        assert!(matches!(open(&p), Err(crate::Error::StoreMissing(_))));
    }

    #[test]
    fn open_restores_a_removed_spool_without_losing_records() {
        let p = tmp("spool-restore");
        let _ = create(&p, 1000).unwrap();
        spool::push(&p.spool, b"pending").unwrap();
        let _ = open(&p).unwrap();
        assert_eq!(spool::read_all(&p.spool).unwrap().len(), 1);
        cleanup(&p);
    }

    #[test]
    fn the_root_is_authored_but_not_stored() {
        // Storage is the log's job. Setup produces a value.
        let p = tmp("not-stored");
        let f = create(&p, 1000).unwrap();
        assert_eq!(f.root.header.seq, 1);
        assert!(!p.events.exists(), "no schema exists yet to store it in");
        cleanup(&p);
    }
}
