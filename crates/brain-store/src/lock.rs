//! One writer per store, elected by the kernel.
//!
//! # What this prevents that the database does not
//!
//! SQLite already stops two transactions interleaving. It does not stop two
//! processes each deciding, independently and correctly, to fold the same
//! events — nor two of them running a rebuild, each writing its own generation
//! and each moving the pointer to it, nor two of them firing the same piece of
//! due work. Those are not races inside a transaction. They are two owners of
//! the same job, and serialising their writes does nothing about it.
//!
//! So one process owns the fold, rebuilds, and the due-work queue. The overlay
//! normally holds it; if nothing does, the first command-line invocation takes
//! it for its lifetime. Everything else opens read-only and submits writes to
//! whoever holds it.
//!
//! # Why an advisory lock rather than a pid file
//!
//! A pid file records an intention. A `flock` records a fact, and the kernel
//! retracts it when the holding process dies — including when it is killed, or
//! when the machine loses power and the file is reopened after reboot.
//!
//! That difference is the whole reason for the choice. A crashed writer leaves
//! a pid file behind, and the next process has to guess whether the pid is a
//! stale entry or a live writer. Guessing wrong in one direction leaves the
//! store permanently unwritable; guessing wrong in the other elects a second
//! writer while the first is still running. Neither is detectable from the file.
//!
//! # The lock is held by a file handle, not by a flag
//!
//! Dropping the [`Writer`] closes the descriptor and releases the lock. There
//! is deliberately no `release()` — a lock that can be released while the value
//! representing it stays alive is a lock that will be.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// Proof that this process owns writing to a store.
///
/// Alive for as long as the ownership is.
#[derive(Debug)]
pub struct Writer {
    _file: File,
    path: PathBuf,
}

impl Writer {
    /// Try to become the writer, without waiting.
    ///
    /// `Ok(None)` means someone else already is — an ordinary outcome, not an
    /// error, because the correct response is to open read-only and hand writes
    /// to the holder.
    ///
    /// `label` is written into the lock file purely so a person can find out
    /// what is holding it. Nothing reads it back for a decision: the lock's
    /// authority comes from the kernel, and a file whose contents were trusted
    /// would be a pid file again.
    pub fn acquire(path: &Path, label: &str) -> Result<Option<Writer>> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;

        match rustix::fs::flock(
            file.as_fd(),
            rustix::fs::FlockOperation::NonBlockingLockExclusive,
        ) {
            Ok(()) => {}
            Err(rustix::io::Errno::WOULDBLOCK) => return Ok(None),
            Err(e) => {
                return Err(Error::Io(std::io::Error::from(e)));
            }
        }

        // Best effort, and deliberately after the lock rather than before: a
        // process that failed to take the lock must not have overwritten the
        // holder's description of itself on the way out.
        let pid = rustix::process::getpid().as_raw_nonzero().get();
        let _ = file.set_len(0);
        let _ = writeln!(file, "{pid} {label}");
        let _ = file.flush();

        Ok(Some(Writer {
            _file: file,
            path: path.to_path_buf(),
        }))
    }

    /// Where the lock lives.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// What the current holder said about itself, for diagnosis only.
///
/// Never used to decide whether the lock is held. It may describe a process
/// that has since exited, which is exactly the failure mode a pid file has and
/// this design avoids by asking the kernel instead.
pub fn holder(path: &Path) -> Option<String> {
    let s = std::fs::read_to_string(path).ok()?;
    let s = s.trim();
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("brain-lock-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("writer.lock")
    }

    fn clean(p: &Path) {
        std::fs::remove_dir_all(p.parent().unwrap()).ok();
    }

    #[test]
    fn the_first_caller_becomes_the_writer() {
        let p = tmp("first");
        let w = Writer::acquire(&p, "overlay").unwrap();
        assert!(w.is_some());
        clean(&p);
    }

    #[test]
    fn a_second_caller_is_told_no_rather_than_failing() {
        // The distinction the return type exists for. Contention is the normal
        // case — three binaries share this store — so it must be expressible
        // without an error, or every read command has to treat "someone else is
        // writing" as a fault.
        let p = tmp("second");
        let _held = Writer::acquire(&p, "overlay").unwrap().unwrap();
        assert!(Writer::acquire(&p, "cli").unwrap().is_none());
        clean(&p);
    }

    #[test]
    fn the_lock_is_released_when_the_holder_is_dropped() {
        let p = tmp("drop");
        {
            let _held = Writer::acquire(&p, "cli").unwrap().unwrap();
            assert!(Writer::acquire(&p, "other").unwrap().is_none());
        }
        assert!(
            Writer::acquire(&p, "other").unwrap().is_some(),
            "ownership must end with the value that represents it"
        );
        clean(&p);
    }

    #[test]
    fn the_holder_describes_itself_for_diagnosis() {
        let p = tmp("holder");
        let _held = Writer::acquire(&p, "overlay").unwrap().unwrap();
        let who = holder(&p).expect("a held lock names its holder");
        assert!(who.contains("overlay"), "got {who:?}");
        assert!(
            who.starts_with(&std::process::id().to_string()),
            "and the process holding it: {who:?}"
        );
        clean(&p);
    }

    #[test]
    fn a_failed_attempt_leaves_the_holders_description_intact() {
        // Writing the description before taking the lock would let a process
        // that never got it overwrite the record of the process that did,
        // leaving a held store with nothing able to say what holds it.
        let p = tmp("no-clobber");
        let _held = Writer::acquire(&p, "overlay").unwrap().unwrap();
        assert!(Writer::acquire(&p, "cli").unwrap().is_none());
        let who = holder(&p).unwrap();
        assert!(who.contains("overlay"), "got {who:?}");
        assert!(!who.contains("cli"));
        clean(&p);
    }

    #[test]
    fn a_lock_file_left_behind_does_not_block_the_next_writer() {
        // The pid-file failure this design avoids: a file that outlives its
        // process must not be mistaken for a live holder.
        let p = tmp("stale");
        std::fs::write(&p, "999999 a process that is long gone\n").unwrap();
        assert!(
            Writer::acquire(&p, "cli").unwrap().is_some(),
            "the kernel, not the file's contents, decides whether it is held"
        );
        clean(&p);
    }
}
