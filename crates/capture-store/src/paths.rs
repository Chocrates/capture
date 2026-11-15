//! Where the store lives.
//!
//! Three binaries have to agree on this, and the test suite has to be able to
//! point somewhere else — without an override, tests would either share your
//! real store or need a mock filesystem.

use std::path::{Path, PathBuf};

/// Environment variable that relocates the entire store.
///
/// Set by the test harness; also useful for running a second isolated store.
pub const STORE_ENV: &str = "CAPTURE_HOME";

/// The agent the command line writes as, when set.
pub const AS_ENV: &str = "CAPTURE_AS";

/// Resolved locations of everything the store owns.
///
/// The three databases are separate files on purpose (`DESIGN.md` D8), split by
/// **derivability** rather than by category:
///
/// - [`events`] is the truth: append-only, immutable, replicated.
/// - [`state`] is derived from it and can be deleted at any time — this is the
///   entire migration strategy, and why no migration code exists.
/// - [`ops`] is local, not derivable, and not replicated.
///
/// The spool is deliberately **not** in any of them. It is a plain preallocated
/// file, because a spool inside SQLite would need the very disk that just filled
/// up, which is the condition it exists to survive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorePaths {
    /// Root directory containing everything below.
    pub root: PathBuf,
    /// Append-only event log. The only durable artifact.
    pub events: PathBuf,
    /// Derived projection. Disposable; rebuilt by re-folding.
    pub state: PathBuf,
    /// Local operational state. Survives a rebuild, never replicated.
    pub ops: PathBuf,
    /// Preallocated capture spool. A plain file, not a database.
    pub spool: PathBuf,
    /// Content-addressed blobs.
    pub blobs: PathBuf,
    /// Incremental event export, for external backup tools.
    pub export: PathBuf,
    /// Device keypair and incarnation. Mode 0600.
    pub identity: PathBuf,
    /// The agent writes are signed as, if not this device (`CAPTURE_AS`).
    ///
    /// Carried here because every write already takes the paths and opens its
    /// own session from them, so this reaches the one place that signs without
    /// a parameter threaded through each of them — and a write that forgot to
    /// pass it on would sign as the person, which is the mistake that matters.
    pub acting: Option<String>,
}

impl StorePaths {
    /// Resolve from the environment: `$CAPTURE_HOME` if set, else `~/capture`.
    ///
    /// Returns `None` only when neither is determinable, which in practice means
    /// no home directory — a case worth reporting rather than guessing at.
    pub fn resolve() -> Option<Self> {
        let root = match std::env::var_os(STORE_ENV) {
            Some(v) if !v.is_empty() => PathBuf::from(v),
            _ => home_dir()?.join("capture"),
        };
        Some(Self::under(root))
    }

    /// Build the layout under an explicit root. Used by tests directly.
    pub fn under(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        Self {
            events: root.join("events.db"),
            state: root.join("state.db"),
            ops: root.join("ops.db"),
            spool: root.join("spool"),
            blobs: root.join("blobs"),
            export: root.join("export"),
            identity: root.join("config").join("identity.key"),
            acting: None,
            root,
        }
    }

    /// The same store, with writes signed as the agent `name`.
    ///
    /// Not read from the environment by [`StorePaths::resolve`]: the overlay
    /// and the sync daemon resolve too, and a stray `CAPTURE_AS` in the shell
    /// that started one must not turn the person's own captures into an
    /// agent's. Only the command line reads [`AS_ENV`].
    pub fn acting_as(mut self, name: Option<String>) -> Self {
        self.acting = name;
        self
    }

    /// Where agent keys live: `config/agents/`.
    pub fn agents_dir(&self) -> PathBuf {
        self.identity.parent().unwrap_or(&self.root).join("agents")
    }

    /// Directories that must exist before anything is written.
    pub fn directories(&self) -> [&Path; 4] {
        [
            self.root.as_path(),
            self.blobs.as_path(),
            self.export.as_path(),
            // identity lives in config/; take its parent rather than hardcoding
            self.identity.parent().unwrap_or(&self.root),
        ]
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// Where the overlay listens for a given store.
///
/// In the session's runtime directory, named from the store it belongs to, so
/// that two stores get two overlays and neither depends on how deeply the store
/// itself happens to be nested. Here rather than in the overlay so that
/// `capture doctor` asks the same address the overlay binds.
pub fn overlay_socket(root: &Path) -> PathBuf {
    let name = format!("capture-{}.sock", socket_tag(root));
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir).join(name),
        // No runtime directory. Falling back to the store keeps the overlay
        // working where one is not set, and `bind` reports the length problem
        // in words if the path turns out to be too long.
        _ => root.join("capture.sock"),
    }
}

/// A short, stable name for a store path.
fn socket_tag(root: &Path) -> String {
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let h = capture_core::id::BodyHash::of(canonical.as_os_str().as_encoded_bytes());
    h.0[..4].iter().map(|b| format!("{b:02x}")).collect()
}

/// The file that makes the overlay start at login, whether or not it exists.
///
/// An XDG autostart entry on Linux, a LaunchAgent on a Mac. Shared with
/// `capture doctor`, which asks whether it is there.
pub fn overlay_login_entry() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        return Some(
            PathBuf::from(std::env::var_os("HOME")?)
                .join("Library/LaunchAgents")
                .join(format!("{}.plist", crate::launchd::OVERLAY)),
        );
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("autostart").join("capture-overlay.desktop"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_rooted_consistently() {
        let p = StorePaths::under("/tmp/example");
        assert_eq!(p.events, Path::new("/tmp/example/events.db"));
        assert_eq!(p.state, Path::new("/tmp/example/state.db"));
        assert_eq!(p.ops, Path::new("/tmp/example/ops.db"));
        assert_eq!(p.identity, Path::new("/tmp/example/config/identity.key"));
    }

    #[test]
    fn spool_is_not_a_database() {
        // Guards DESIGN.md §7 mode 6: a spool inside SQLite would need the disk
        // that just filled up.
        let p = StorePaths::under("/tmp/example");
        assert_eq!(p.spool, Path::new("/tmp/example/spool"));
        assert_ne!(p.spool, p.ops);
        assert!(p.spool.extension().is_none());
    }

    #[test]
    fn directories_include_config_parent() {
        let p = StorePaths::under("/tmp/example");
        let dirs = p.directories();
        assert!(dirs.contains(&Path::new("/tmp/example/config")));
    }
}
