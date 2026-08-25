//! What was going on when a capture happened.
//!
//! # Context is the recall cue when the words are not
//!
//! A note reading "ask about the migration" is unfindable by content six weeks
//! later. "In `~/work/api` on branch `auth-rewrite`, on the Tuesday" is often
//! enough on its own. That is the entire justification for collecting any of
//! this: it costs the user nothing at capture time, and it is frequently the
//! only handle left.
//!
//! # Absent is a value, not a gap to fill
//!
//! Every field is optional, and an unavailable field stays `None`. The
//! temptation is to substitute something plausible — the empty string, a
//! placeholder, the last known value — and each of those turns "we could not
//! see this" into "this is what it was", which a query cannot tell apart.
//!
//! The Xwayland case is the sharp one and is refused explicitly below.
//!
//! # The clipboard is never read
//!
//! Not "read but not stored" — never read. Copying the clipboard into an
//! append-only log is precisely how a credential gets into it: the case
//! redaction exists for, arriving by the one route that also reaches written
//! exports, where redaction can never follow. There is no setting for this.

use std::path::Path;

use brain_core::body::Ambient;

/// Where the ambient facts come from.
///
/// A trait so the collector can be exercised without a desktop session, and so
/// the honest answer on a platform that hides something is a type that returns
/// `None` rather than a comment saying it does.
pub trait Environment {
    /// The process's working directory.
    fn cwd(&self) -> Option<String>;
    /// The foreground application's identity, if the platform exposes it.
    fn foreground_app(&self) -> Option<String>;
    /// The foreground window's title, if the platform exposes it.
    fn foreground_title(&self) -> Option<String>;
}

/// Collect what this machine will actually tell us.
pub fn collect(env: &dyn Environment) -> Ambient {
    let cwd = env.cwd();
    let (repo, branch) = match cwd.as_deref() {
        Some(dir) => git(Path::new(dir)),
        None => (None, None),
    };
    Ambient {
        app: env.foreground_app(),
        window_title: env.foreground_title(),
        cwd,
        repo,
        branch,
        source_url: None,
    }
}

/// The repository name and current branch, read from the filesystem.
///
/// Deliberately not by running `git`. Capture must be fast and must work when
/// the machine is busy, and spawning a process on the capture path makes it
/// depend on the process table, `PATH`, and whatever a shell profile does.
/// Reading two files does not.
fn git(from: &Path) -> (Option<String>, Option<String>) {
    let Some(root) = find_git_dir(from) else {
        return (None, None);
    };

    // The repository's name is its working tree's directory name. `.git` may
    // be a file in a worktree or submodule, so the parent is what to name.
    let repo = root
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string());

    // HEAD is either a symbolic ref, or a bare commit id when detached.
    // Detached is reported as absent rather than as a forty-character hex
    // string, which is not a branch and would only ever be noise in a view.
    let branch = std::fs::read_to_string(root.join("HEAD"))
        .ok()
        .and_then(|h| {
            h.trim()
                .strip_prefix("ref: refs/heads/")
                .map(|b| b.to_string())
        });

    (repo, branch)
}

/// Walk up looking for a `.git`, the way git itself does.
fn find_git_dir(from: &Path) -> Option<std::path::PathBuf> {
    let mut dir = Some(from);
    while let Some(d) = dir {
        let candidate = d.join(".git");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if candidate.is_file() {
            // A worktree or submodule: the file points at the real directory.
            let text = std::fs::read_to_string(&candidate).ok()?;
            let path = text.trim().strip_prefix("gitdir: ")?;
            let p = std::path::PathBuf::from(path);
            return Some(if p.is_absolute() { p } else { d.join(p) });
        }
        dir = d.parent();
    }
    None
}

/// This machine, asked honestly.
pub struct Host;

impl Environment for Host {
    fn cwd(&self) -> Option<String> {
        std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().to_string())
    }

    /// # Why this returns `None` here
    ///
    /// On Wayland there is no protocol that tells a client which window has
    /// focus — that is a deliberate isolation property, not an omission. GNOME's
    /// `Shell.Eval` escape hatch is disabled outside developer mode.
    ///
    /// Xwayland is the trap. `_NET_ACTIVE_WINDOW` still exists and still
    /// answers, and on this target it answers `0x0` — a well-formed reply
    /// meaning "no window", indistinguishable at a glance from a real one. A
    /// collector that trusted it would record every capture as having happened
    /// in the same nonexistent window, and the field would look populated while
    /// being uniformly wrong. Refusing it is the only honest option.
    ///
    /// Under a genuine X11 session this is answerable, and a backend for it
    /// belongs here when one exists. It is not implemented as a guess in the
    /// meantime.
    fn foreground_app(&self) -> Option<String> {
        None
    }

    fn foreground_title(&self) -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An environment that answers exactly what a test says it does.
    struct Fake {
        cwd: Option<String>,
        app: Option<String>,
        title: Option<String>,
    }

    impl Environment for Fake {
        fn cwd(&self) -> Option<String> {
            self.cwd.clone()
        }
        fn foreground_app(&self) -> Option<String> {
            self.app.clone()
        }
        fn foreground_title(&self) -> Option<String> {
            self.title.clone()
        }
    }

    fn nothing() -> Fake {
        Fake {
            cwd: None,
            app: None,
            title: None,
        }
    }

    #[test]
    fn an_environment_that_knows_nothing_yields_nothing() {
        // Every field absent, and none of them substituted with a plausible
        // value. A query can tell "not observed" from "observed to be empty"
        // only if the first stays absent.
        let a = collect(&nothing());
        assert_eq!(a.cwd, None);
        assert_eq!(a.app, None);
        assert_eq!(a.window_title, None);
        assert_eq!(a.repo, None);
        assert_eq!(a.branch, None);
    }

    #[test]
    fn a_directory_outside_a_repository_has_no_repo_or_branch() {
        let d = std::env::temp_dir().join(format!("brain-amb-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let a = collect(&Fake {
            cwd: Some(d.to_string_lossy().to_string()),
            ..nothing()
        });
        assert_eq!(a.cwd.as_deref(), Some(d.to_string_lossy().as_ref()));
        assert_eq!(a.repo, None);
        assert_eq!(a.branch, None);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_repository_yields_its_name_and_branch() {
        let d = std::env::temp_dir().join(format!("brain-amb-repo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let work = d.join("my-project");
        std::fs::create_dir_all(work.join(".git")).unwrap();
        std::fs::write(
            work.join(".git").join("HEAD"),
            "ref: refs/heads/auth-rewrite\n",
        )
        .unwrap();

        let a = collect(&Fake {
            cwd: Some(work.to_string_lossy().to_string()),
            ..nothing()
        });
        assert_eq!(a.repo.as_deref(), Some("my-project"));
        assert_eq!(a.branch.as_deref(), Some("auth-rewrite"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_subdirectory_finds_the_repository_above_it() {
        let d = std::env::temp_dir().join(format!("brain-amb-sub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let work = d.join("my-project");
        let deep = work.join("src").join("inner");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(work.join(".git")).unwrap();
        std::fs::write(work.join(".git").join("HEAD"), "ref: refs/heads/main\n").unwrap();

        let a = collect(&Fake {
            cwd: Some(deep.to_string_lossy().to_string()),
            ..nothing()
        });
        assert_eq!(a.repo.as_deref(), Some("my-project"));
        assert_eq!(a.branch.as_deref(), Some("main"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_detached_head_reports_no_branch() {
        // A commit id is not a branch. Recording one would put a
        // forty-character hex string where a view expects a name.
        let d = std::env::temp_dir().join(format!("brain-amb-detached-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let work = d.join("my-project");
        std::fs::create_dir_all(work.join(".git")).unwrap();
        std::fs::write(
            work.join(".git").join("HEAD"),
            "9f2c1a4e5b6d7c8f9a0b1c2d3e4f5a6b7c8d9e0f\n",
        )
        .unwrap();

        let a = collect(&Fake {
            cwd: Some(work.to_string_lossy().to_string()),
            ..nothing()
        });
        assert_eq!(a.repo.as_deref(), Some("my-project"));
        assert_eq!(a.branch, None);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_worktree_pointer_is_followed() {
        // `.git` as a file rather than a directory. Not following it would
        // report every worktree as being outside a repository.
        let d = std::env::temp_dir().join(format!("brain-amb-wt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let real = d.join("real-git");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("HEAD"), "ref: refs/heads/feature\n").unwrap();

        let work = d.join("checkout");
        std::fs::create_dir_all(&work).unwrap();
        std::fs::write(
            work.join(".git"),
            format!("gitdir: {}\n", real.to_string_lossy()),
        )
        .unwrap();

        let a = collect(&Fake {
            cwd: Some(work.to_string_lossy().to_string()),
            ..nothing()
        });
        assert_eq!(a.branch.as_deref(), Some("feature"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn this_platform_does_not_claim_to_know_the_foreground_window() {
        // The honest answer on the target. Xwayland will happily return 0x0 for
        // `_NET_ACTIVE_WINDOW` — a well-formed reply meaning "no window" — and
        // recording it would fill the field with a value that is uniformly
        // wrong rather than leaving it visibly absent.
        assert_eq!(Host.foreground_app(), None);
        assert_eq!(Host.foreground_title(), None);
    }

    #[test]
    fn the_collector_has_no_way_to_read_a_clipboard() {
        // Enforced by the shape of the trait rather than by discipline: there
        // is no method to implement, so no backend can add one without changing
        // this file. Copying a clipboard into an append-only log is how a
        // credential gets somewhere redaction cannot follow.
        let a = collect(&nothing());
        let encoded = format!("{a:?}");
        assert!(!encoded.contains("clip"), "{encoded}");
    }
}
