//! The capture spool: a preallocated file that keeps capture working when the
//! main store cannot be written.
//!
//! # Why this is a file and not a database table
//!
//! The condition this exists to survive is **the disk being full**. A table
//! inside the store would need to allocate pages on exactly the disk that just
//! ran out, which is the one thing it cannot do. So the space is claimed up
//! front, while there is definitely room, and the spool writes into space it
//! already owns.
//!
//! # Why it is not a ring buffer
//!
//! A ring buffer overwrites its oldest entries when full. That is the correct
//! structure for telemetry and the wrong one here: the promise is that a
//! capture is never silently discarded. When this fills, capture **fails
//! loudly** instead — a visible refusal is recoverable, a silent drop is not.
//!
//! # Why sequence numbers are not assigned here
//!
//! Each origin numbers its events consecutively with no gaps. If a number were
//! stamped when a capture entered the spool and the drain later completed only
//! part-way — because the process was killed, or the disk filled again — the
//! log would be left with a hole that nothing can fill. So the spool stores
//! captures in arrival order with no numbering, and numbers are assigned as
//! each one is committed.
//!
//! # Record format
//!
//! Length-prefixed, in order, from the start of the file:
//!
//! ```text
//! [u32 length][payload bytes][u32 length][payload bytes]...[u32 zero]
//! ```
//!
//! A zero length marks the end. Because the file is preallocated with zeroes, a
//! fresh spool already reads as empty, and a torn write at the tail reads as
//! the end of the records rather than as corruption.
//!
//! That last property is a consequence of the *write order*, not of the format,
//! and it did not hold when the length was written first. Preallocation means
//! the payload's bytes are already there and already readable: a crash between
//! the length landing and the payload landing left a record whose length was
//! durable and whose body was ten thousand zeroes, and nothing downstream could
//! tell that from a capture. It was hashed, signed and appended to the log —
//! the one artifact in the system that is never rewritten.
//!
//! So the payload and its terminator are written and synced first, and the
//! length last. Until that length is durable the record does not exist, and
//! after it is, everything it points at already does.

use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::{Error, Result};

/// Preallocated size. Roughly 50,000 text captures.
///
/// Capacity is not really the protection here — a full spool means something
/// has been wrong for a long time, and the actual defence is that the condition
/// is reported the moment the spool is first used. This size just makes it
/// implausible to hit before anyone notices.
pub const SPOOL_BYTES: u64 = 10 * 1024 * 1024;

/// A capture waiting to be committed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpooledCapture {
    /// Encoded capture payload.
    pub payload: Vec<u8>,
}

/// Claim the spool's space on disk.
///
/// Done at install, while there is definitely room. Writing zeroes rather than
/// merely setting the file length matters: a sparse file reserves nothing, and
/// would fail at exactly the moment it is needed.
pub fn preallocate(path: &Path) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if path.exists() && std::fs::metadata(path)?.len() >= SPOOL_BYTES {
        return Ok(());
    }
    let mut f = File::create(path)?;
    let chunk = vec![0u8; 64 * 1024];
    let mut written = 0u64;
    while written < SPOOL_BYTES {
        let n = chunk.len().min((SPOOL_BYTES - written) as usize);
        f.write_all(&chunk[..n])?;
        written += n as u64;
    }
    f.sync_all()?;
    Ok(())
}

/// Append a capture. Fails loudly when there is no room left.
pub fn push(path: &Path, payload: &[u8]) -> Result<()> {
    let mut f = OpenOptions::new().read(true).write(true).open(path)?;
    let end = scan_end(&mut f)?;
    let need = 4 + payload.len() as u64 + 4; // record, plus the terminator

    if end + need > SPOOL_BYTES {
        return Err(Error::Io(std::io::Error::other(
            "capture spool is full — captures cannot be accepted until the disk recovers",
        )));
    }

    // Payload first, length last, with a sync between them. See the module
    // header: written the other way round, a crash in the gap leaves a record
    // that is complete as far as any reader can tell and is entirely zeroes.
    //
    // The terminator goes down in the first pass too. `clear` only zeroes the
    // first length, so the bytes after a reused spool's new record are whatever
    // the previous contents left there, and the record needs an end of its own.
    f.seek(SeekFrom::Start(end + 4))?;
    f.write_all(payload)?;
    f.write_all(&0u32.to_le_bytes())?;
    f.sync_all()?;

    // Now the record exists. The spool exists because the ordinary durable path
    // is unavailable; a capture it accepted must survive whatever happens next.
    f.seek(SeekFrom::Start(end))?;
    f.write_all(&(payload.len() as u32).to_le_bytes())?;
    f.sync_all()?;
    Ok(())
}

/// Read every spooled capture, oldest first. Does not remove anything.
pub fn read_all(path: &Path) -> Result<Vec<SpooledCapture>> {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(_) => return Ok(Vec::new()),
    };
    let mut out = Vec::new();
    let mut pos = 0u64;
    loop {
        f.seek(SeekFrom::Start(pos))?;
        let mut len = [0u8; 4];
        if f.read_exact(&mut len).is_err() {
            break;
        }
        let n = u32::from_le_bytes(len) as usize;
        if n == 0 || pos + 4 + n as u64 > SPOOL_BYTES {
            break;
        }
        let mut buf = vec![0u8; n];
        if f.read_exact(&mut buf).is_err() {
            // Torn tail: the record was never completed, so it was never
            // acknowledged either. Stopping here loses nothing.
            break;
        }
        // Belt as well as braces, for the one window the write order does not
        // close: the length itself is four bytes and a crash can tear those,
        // leaving a plausible count over preallocated zeroes.
        //
        // An encoded capture is never all zeroes — it is CBOR, and its first
        // byte is a map or array header. Reading zeroes as a capture is how a
        // signed record of nothing reaches the append-only log, so they end the
        // scan like any other torn tail.
        if buf.iter().all(|b| *b == 0) {
            break;
        }
        out.push(SpooledCapture { payload: buf });
        pos += 4 + n as u64;
    }
    Ok(out)
}

/// Whether the spool currently holds anything.
///
/// This is the trigger for the condition report — the spool being *in use* is
/// what needs surfacing, not the spool being nearly full.
pub fn in_use(path: &Path) -> Result<bool> {
    Ok(!read_all(path)?.is_empty())
}

/// Discard everything, after a successful drain.
pub fn clear(path: &Path) -> Result<()> {
    let mut f = OpenOptions::new().write(true).open(path)?;
    f.seek(SeekFrom::Start(0))?;
    f.write_all(&0u32.to_le_bytes())?;
    f.sync_all()?;
    Ok(())
}

fn scan_end(f: &mut File) -> Result<u64> {
    let mut pos = 0u64;
    loop {
        f.seek(SeekFrom::Start(pos))?;
        let mut len = [0u8; 4];
        if f.read_exact(&mut len).is_err() {
            return Ok(pos);
        }
        let n = u32::from_le_bytes(len) as u64;
        if n == 0 || pos + 4 + n > SPOOL_BYTES {
            return Ok(pos);
        }
        pos += 4 + n;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("capture-spool-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d.join("spool")
    }

    #[test]
    fn preallocation_claims_real_space() {
        // Not a sparse file: a hole reserves nothing and would fail at the one
        // moment the spool is needed.
        let p = tmp("prealloc");
        preallocate(&p).unwrap();
        assert_eq!(std::fs::metadata(&p).unwrap().len(), SPOOL_BYTES);
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn a_fresh_spool_reads_as_empty() {
        let p = tmp("fresh");
        preallocate(&p).unwrap();
        assert!(read_all(&p).unwrap().is_empty());
        assert!(!in_use(&p).unwrap());
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    /// A torn record is not a capture.
    ///
    /// The failure this guards against is not a corrupt read — it is a
    /// perfectly successful one. The spool is preallocated with zeroes, so
    /// every byte a length points at is already there and already readable:
    /// a length that became durable without its payload yields a record that
    /// `read_exact` completes, `drain` hashes, `author` signs and `log.append`
    /// writes into the one file in the system that is never rewritten.
    ///
    /// Reproduced here by writing what the old order left behind — the length,
    /// and nothing after it.
    #[test]
    fn a_torn_length_does_not_become_an_all_zero_capture() {
        let p = tmp("torn");
        preallocate(&p).unwrap();
        push(&p, &[0xa1, 0x01, 0x02]).unwrap();

        let mut f = OpenOptions::new().read(true).write(true).open(&p).unwrap();
        let end = scan_end(&mut f).unwrap();
        f.seek(SeekFrom::Start(end)).unwrap();
        f.write_all(&64u32.to_le_bytes()).unwrap();
        f.sync_all().unwrap();

        let back = read_all(&p).unwrap();
        assert_eq!(back.len(), 1, "a torn record was read back as a capture");
        assert_eq!(back[0].payload, vec![0xa1, 0x01, 0x02]);
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    /// The length is the last thing to become durable.
    ///
    /// Stated as the property rather than the byte order, because the property
    /// is what makes a torn write survivable: at every instant during a push,
    /// either the record is absent or everything it points at is already on
    /// disk. A reader can therefore never see a length without a payload.
    #[test]
    fn a_record_is_durable_before_it_is_visible() {
        let p = tmp("durable-order");
        preallocate(&p).unwrap();

        let mut f = OpenOptions::new().read(true).write(true).open(&p).unwrap();
        let end = scan_end(&mut f).unwrap();

        // The payload, written where a push would put it, with no length yet.
        f.seek(SeekFrom::Start(end + 4)).unwrap();
        f.write_all(&[0xa1, 0x01, 0x02]).unwrap();
        f.sync_all().unwrap();

        assert!(
            read_all(&p).unwrap().is_empty(),
            "a payload with no length was visible as a record"
        );
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn captures_come_back_in_order() {
        let p = tmp("order");
        preallocate(&p).unwrap();
        push(&p, b"first").unwrap();
        push(&p, b"second").unwrap();
        push(&p, b"third").unwrap();
        let got: Vec<_> = read_all(&p)
            .unwrap()
            .into_iter()
            .map(|c| c.payload)
            .collect();
        assert_eq!(
            got,
            vec![b"first".to_vec(), b"second".to_vec(), b"third".to_vec()]
        );
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn in_use_is_the_signal_that_something_is_wrong() {
        let p = tmp("inuse");
        preallocate(&p).unwrap();
        assert!(!in_use(&p).unwrap());
        push(&p, b"x").unwrap();
        assert!(in_use(&p).unwrap());
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn clearing_empties_it() {
        let p = tmp("clear");
        preallocate(&p).unwrap();
        push(&p, b"x").unwrap();
        clear(&p).unwrap();
        assert!(read_all(&p).unwrap().is_empty());
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn a_full_spool_fails_loudly_rather_than_overwriting() {
        // The ring-buffer alternative would silently discard the oldest
        // capture here. The promise is that nothing is silently discarded.
        let p = tmp("full");
        preallocate(&p).unwrap();
        let big = vec![7u8; 1024 * 1024];
        let mut pushed = 0;
        loop {
            match push(&p, &big) {
                Ok(()) => pushed += 1,
                Err(e) => {
                    assert!(format!("{e}").contains("full"));
                    break;
                }
            }
            assert!(pushed < 20, "should have filled before this");
        }
        // Everything accepted is still there — nothing was overwritten.
        assert_eq!(read_all(&p).unwrap().len(), pushed);
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn spool_survives_reopen() {
        let p = tmp("reopen");
        preallocate(&p).unwrap();
        push(&p, b"persisted").unwrap();
        // Re-running preallocate must not clobber an existing spool.
        preallocate(&p).unwrap();
        assert_eq!(read_all(&p).unwrap().len(), 1);
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }
}
