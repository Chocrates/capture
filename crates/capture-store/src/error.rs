//! Errors, and the exit codes they become.
//!
//! Defined now rather than later because the CLI promises four distinct exit
//! codes, and retrofitting a typed error onto `anyhow` once every call site
//! exists is a large, joyless refactor.
//!
//! The mapping is the point. A caller distinguishing "you typed it wrong" from
//! "that note does not exist" from "the log is damaged" needs those to be
//! different kinds all the way down, not a string inspected at the top.

use std::path::PathBuf;

/// Process exit codes. Stable; scripts depend on them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum ExitCode {
    /// Completed.
    Success = 0,
    /// The invocation was wrong — bad flag, bad argument, contradictory options.
    Usage = 1,
    /// The thing asked for does not exist.
    NotFound = 2,
    /// Stored data failed verification. Distinct from every other failure
    /// because it means the archive is damaged rather than the request.
    Integrity = 3,
}

/// Anything that can go wrong below the interface layer.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The store is not where it was expected, or cannot be located at all.
    #[error("no store at {0}")]
    StoreMissing(PathBuf),

    /// The named note, event or slug does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// An argument or flag combination is not valid.
    #[error("{0}")]
    Usage(String),

    /// Stored data does not match what it commits to. This is the one that
    /// means the archive is damaged rather than the request being wrong, and it
    /// is why `Integrity` is a separate exit code.
    #[error("integrity failure: {0}")]
    Integrity(String),

    /// Underlying storage failed.
    #[error("storage: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// Underlying filesystem failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// A peer was there and stopped making progress.
    ///
    /// Separate from `Usage` because it is not a fault and should not read like
    /// one: a phone leaving Wi-Fi range mid-round is the single most ordinary
    /// event in a two-device system, and the honest response is to say so and
    /// try again on the next sweep. Anti-entropy is resumable, so nothing is
    /// lost by the round ending where it did.
    #[error("{0}")]
    Unreachable(String),

    /// The projection was built by an older build and is the wrong shape to
    /// read. Not damage: it is derived, and anything that opens the archive
    /// for writing rebuilds it.
    #[error(
        "this archive's index was built by an older version of capture (shape {found}, \
         and this one reads {want}).\nAnything that writes rebuilds it:  capture settle",
        want = crate::projection::PROJECTION_SCHEMA
    )]
    Stale { found: u32 },
}

impl Error {
    /// The exit code this error should produce.
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Error::Usage(_) => ExitCode::Usage,
            Error::NotFound(_) | Error::StoreMissing(_) => ExitCode::NotFound,
            Error::Integrity(_) => ExitCode::Integrity,
            // Storage and IO failures are not usage errors and not integrity
            // failures — they are the environment failing underneath us.
            Error::Sqlite(_) | Error::Io(_) => ExitCode::Usage,
            // Not a usage error and not a broken archive: the network is the
            // network. `capture sync` exits non-zero so a script can retry,
            // which is the same code an unreachable peer already produced.
            Error::Unreachable(_) => ExitCode::Usage,
            // The environment again, and fixed by running something.
            Error::Stale { .. } => ExitCode::Usage,
        }
    }
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_distinct_per_class() {
        assert_eq!(Error::Usage("x".into()).exit_code(), ExitCode::Usage);
        assert_eq!(Error::NotFound("x".into()).exit_code(), ExitCode::NotFound);
        assert_eq!(
            Error::Integrity("x".into()).exit_code(),
            ExitCode::Integrity
        );
    }

    #[test]
    fn integrity_is_not_conflated_with_usage() {
        // The distinction that matters: a damaged archive must never be
        // reported the same way as a mistyped flag.
        assert_ne!(
            Error::Integrity("x".into()).exit_code(),
            Error::Usage("x".into()).exit_code()
        );
    }

    #[test]
    fn success_is_zero() {
        assert_eq!(ExitCode::Success as i32, 0);
    }
}
