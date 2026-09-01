//! Bytes held outside the log, named by what they contain (§5.6).
//!
//! # Why the content decides the name
//!
//! The hash is the address. Verification is then free — a device that fetched
//! the wrong bytes cannot fail to notice, because the name it asked for is a
//! claim about the content it got — and duplicates are impossible: the same
//! recording captured twice is one file with two references.
//!
//! # Why they are not in the log
//!
//! The log is replicated in full to every device and is meant to be worth
//! reading in ten years. Forty megabytes of audio inside an event would reach
//! every replica whether it wanted the recording or not, and a phone could
//! never drop it without dropping the note as well. Keeping the reference in
//! the event and the bytes beside it is what makes eviction possible at all: a
//! constrained device drops the bytes and keeps everything that made the note
//! findable.
//!
//! # Three ways bytes go away, and only one of them loses anything
//!
//! §5.6 is emphatic that these were once conflated, and that collapsing them
//! is how "freed some space" silently becomes "lost the recording".
//!
//! - **Evict** — one device drops its copy. Refetchable from any full replica.
//!   This is what a phone does, aggressively, and it costs nothing.
//! - **Expire** — every replica drops it, unrecoverably. **Removed from the
//!   design**, and so absent here: a retention window was a `setting`, which
//!   is last-writer-wins and eventually consistent, so replicas would have
//!   destroyed different sets of bytes at different moments with no event and
//!   no audit trail.
//! - **Redact** — deliberate, bounded, and unrecoverable by design (§1.1).
//!   The only path that destroys a blob everywhere.
//!
//! So this module offers `evict` and `destroy`, and no third thing. There is
//! nowhere to add a policy that quietly deletes.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// How the world writes a blob's name.
///
/// Prefixed, because a bare hash says nothing about which hash. When a second
/// one is ever needed, an unprefixed name would already be ambiguous in every
/// archive in existence.
pub fn name(hash: &[u8; 32]) -> String {
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    format!("b3:{hex}")
}

/// Read a name back, if it is one.
pub fn parse(name: &str) -> Option<[u8; 32]> {
    let hex = name.strip_prefix("b3:")?;
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

/// Where one blob lives.
///
/// Two levels of fan-out, because a directory with a hundred thousand entries
/// is slow to list on every filesystem that has ever shipped and this costs
/// two bytes of path to avoid.
fn path_of(blobs: &Path, hash: &[u8; 32]) -> PathBuf {
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    blobs.join(&hex[0..2]).join(&hex[2..4]).join(&hex)
}

/// Store bytes, returning what they are called.
///
/// Written to a temporary file and renamed, so a reader never sees a partial
/// blob under a name that promises whole content. On the same filesystem the
/// rename is atomic, and the name is a hash: a half-written file under it
/// would be a file whose name is a lie, which is exactly what content
/// addressing exists to make impossible.
pub fn put(blobs: &Path, bytes: &[u8]) -> Result<[u8; 32]> {
    let hash: [u8; 32] = *blake3::hash(bytes).as_bytes();
    let final_path = path_of(blobs, &hash);

    // Already here. Writing it again would be identical work for an identical
    // result, and the same recording captured twice is the ordinary case.
    if final_path.exists() {
        return Ok(hash);
    }

    let dir = final_path.parent().expect("a blob path has a parent");
    std::fs::create_dir_all(dir)?;

    let temporary = dir.join(format!(".{}.part", name(&hash)));
    {
        let mut f = std::fs::File::create(&temporary)?;
        f.write_all(bytes)?;
        // Durable before it is visible. A rename that lands before the
        // contents do would leave a name promising bytes that a crash took
        // away, and the name is the only check anybody has.
        f.sync_all()?;
    }
    std::fs::rename(&temporary, &final_path)?;
    Ok(hash)
}

/// Read bytes back, checking they are what was asked for.
///
/// The check is not belt-and-braces. Content addressing's whole claim is that
/// a wrong answer is detectable, and it is only true if somebody looks.
pub fn get(blobs: &Path, hash: &[u8; 32]) -> Result<Option<Vec<u8>>> {
    let path = path_of(blobs, hash);
    if !path.exists() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&path)?.read_to_end(&mut bytes)?;

    if blake3::hash(&bytes).as_bytes() != hash {
        // The file is not what its name says. Refusing is the only safe
        // answer: handing these back would let corruption travel to every
        // replica that fetches from this one, under a name that vouches for
        // it.
        return Err(Error::Usage(format!(
            "{} does not contain what its name says",
            name(hash)
        )));
    }
    Ok(Some(bytes))
}

/// Whether this device holds the bytes.
pub fn have(blobs: &Path, hash: &[u8; 32]) -> bool {
    path_of(blobs, hash).exists()
}

/// How much of it this device holds, for resuming a transfer.
pub fn have_bytes(blobs: &Path, hash: &[u8; 32]) -> u64 {
    std::fs::metadata(path_of(blobs, hash))
        .map(|m| m.len())
        .unwrap_or(0)
}

/// Drop this device's copy, keeping the note.
///
/// Not destruction: the bytes are refetchable from any replica that still has
/// them, and the reference in the log is untouched. This is what a phone does
/// when it runs short of room, and it is safe precisely because it is not the
/// same operation as redaction.
pub fn evict(blobs: &Path, hash: &[u8; 32]) -> Result<()> {
    let path = path_of(blobs, hash);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Destroy the bytes, deliberately and unrecoverably (§1.1).
///
/// Overwritten before unlinking, so the content does not survive in a freed
/// block that something else reads later. This is best-effort against a
/// journalling filesystem or an SSD's remapping, and it is still worth doing:
/// the alternative leaves the bytes intact by construction rather than by
/// accident.
pub fn destroy(blobs: &Path, hash: &[u8; 32]) -> Result<()> {
    let path = path_of(blobs, hash);
    let Ok(meta) = std::fs::metadata(&path) else {
        return Ok(());
    };

    if let Ok(mut f) = std::fs::OpenOptions::new().write(true).open(&path) {
        let zeros = vec![0u8; 64 * 1024];
        let mut left = meta.len();
        while left > 0 {
            let n = left.min(zeros.len() as u64) as usize;
            if f.write_all(&zeros[..n]).is_err() {
                break;
            }
            left -= n as u64;
        }
        let _ = f.sync_all();
    }
    std::fs::remove_file(&path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "capture-blobs-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_same_bytes_are_the_same_blob() {
        // Duplicates are impossible rather than deduplicated. Nothing here
        // compares anything: the name is derived from the content, so the
        // second write finds the first already there.
        let d = scratch("same");
        let a = put(&d, b"a recording").unwrap();
        let b = put(&d, b"a recording").unwrap();
        assert_eq!(a, b);
        assert_eq!(get(&d, &a).unwrap().as_deref(), Some(&b"a recording"[..]));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn bytes_that_are_not_what_the_name_says_are_refused() {
        // The whole claim of content addressing is that a wrong answer is
        // detectable, and it is only true if somebody looks. Without this
        // check a corrupt file travels to every replica that fetches from
        // here, under a name that vouches for it.
        let d = scratch("corrupt");
        let hash = put(&d, b"a recording").unwrap();
        std::fs::write(path_of(&d, &hash), b"something else entirely").unwrap();

        assert!(
            get(&d, &hash).is_err(),
            "a file whose name is a lie is not served"
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn eviction_keeps_the_note_and_destruction_does_not_keep_the_bytes() {
        // §5.6 says these were once one operation, and that collapsing them is
        // how "freed some space" silently becomes "lost the recording". The
        // difference is not in what the disk looks like afterwards — both
        // leave no file — it is that one is refetchable and the other is
        // deliberately not, and only the caller knows which it meant.
        let d = scratch("evict");
        let hash = put(&d, b"a recording").unwrap();

        evict(&d, &hash).unwrap();
        assert!(!have(&d, &hash), "the bytes are gone from here");
        // And can come back, because eviction destroyed nothing anybody else
        // holds.
        assert_eq!(put(&d, b"a recording").unwrap(), hash);
        assert!(have(&d, &hash));

        destroy(&d, &hash).unwrap();
        assert!(!have(&d, &hash));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn asking_for_something_never_stored_is_not_an_error() {
        // A device that has evicted a blob is in exactly this state, and it is
        // the ordinary case rather than a fault: the answer is "not here",
        // and the caller fetches it from somebody who has it.
        let d = scratch("absent");
        let missing = [0x11; 32];
        assert_eq!(get(&d, &missing).unwrap(), None);
        assert!(!have(&d, &missing));
        assert_eq!(have_bytes(&d, &missing), 0);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_name_survives_being_written_down_and_read_back() {
        let hash = [0x9d; 32];
        assert_eq!(parse(&name(&hash)), Some(hash));
        assert_eq!(
            parse("9d9d9d"),
            None,
            "a bare hash says nothing about which"
        );
        assert_eq!(parse("b3:short"), None);
        assert_eq!(
            parse("sha256:{}"),
            None,
            "and a different one is not this one"
        );
    }
}
