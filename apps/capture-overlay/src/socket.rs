//! The one-word protocol between the chord and the overlay.
//!
//! # Why a socket rather than starting the application
//!
//! The desktop runs a command when the chord is pressed. If that command were
//! the overlay, every capture would pay for process start, dynamic linking,
//! font loading and window creation — hundreds of milliseconds, at the exact
//! moment the gesture has to feel like it already happened.
//!
//! So the overlay is resident with its window built and hidden, and the chord
//! runs a client that connects, writes one word, and exits. That client does
//! nothing else, which is what makes it fast enough to be spawned.
//!
//! # A socket file that outlives its process
//!
//! A unix socket leaves a file behind when its owner dies. The next overlay to
//! start finds it, and binding fails with "address already in use" — a message
//! that says the opposite of what is true: nothing is listening.
//!
//! Deleting it unconditionally is worse, because that is exactly how two
//! overlays end up running with the second one stealing the first one's chord.
//!
//! The distinction is drawn by *asking*: connect to it first. A refused
//! connection means the file is stale and can be removed. A successful one
//! means somebody is home, and this process should not be starting.
//!
//! # Why it does not live in the store directory
//!
//! Unix socket paths are limited to about 108 bytes — the size of a field in a
//! kernel struct, not a filesystem limit — and the store may be anywhere. Under
//! a long `$CAPTURE_HOME` the overlay simply fails to start, with a message
//! (`path must be shorter than SUN_LEN`) that explains nothing to the person
//! reading it. This was not hypothetical: it happened the first time the
//! overlay was run from a scratch directory.
//!
//! So the socket goes in the session's runtime directory, which is short by
//! construction (`/run/user/1000`), and is named from a hash of the store path
//! so two stores never collide. That directory is also cleared when the session
//! ends, which disposes of stale sockets without anybody having to think about
//! it — a socket is runtime state, and it was only ever in the store directory
//! because that was where the other paths were.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

use capture_store::{Error, Result};

/// The only message.
///
/// One word, and no arguments. Anything richer would be a protocol to keep
/// compatible across versions, for a channel whose entire job is to say "now".
const SHOW: &[u8] = b"show\n";

/// The longest a unix socket path may be, with room for the terminator.
///
/// The limit comes from `sockaddr_un.sun_path`, which is 108 bytes on Linux.
const MAX_SOCKET_PATH: usize = 100;

/// Where the overlay listens for a given store.
///
/// In the session's runtime directory, named from the store it belongs to, so
/// that two stores get two overlays and neither depends on how deeply the store
/// itself happens to be nested.
pub fn path(root: &Path) -> PathBuf {
    let name = format!("capture-{}.sock", tag(root));
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir).join(name),
        // No runtime directory. Falling back to the store keeps the overlay
        // working where one is not set, and `bind` reports the length problem
        // in words if the path turns out to be too long.
        _ => root.join("capture.sock"),
    }
}

/// A short, stable name for a store path.
fn tag(root: &Path) -> String {
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let h = capture_core::id::BodyHash::of(canonical.as_os_str().as_encoded_bytes());
    h.0[..4].iter().map(|b| format!("{b:02x}")).collect()
}

/// Ask a running overlay to show itself.
///
/// Returns `Ok(false)` when nothing is listening — an ordinary answer, not a
/// failure. The chord may well be pressed when the overlay is not running, and
/// the caller decides what to do about that.
pub fn signal(root: &Path) -> Result<bool> {
    match UnixStream::connect(path(root)) {
        Ok(mut s) => {
            s.write_all(SHOW)?;
            s.flush()?;
            Ok(true)
        }
        Err(_) => Ok(false),
    }
}

/// Refuse a path the kernel cannot hold, in words rather than in errno.
fn check_length(p: &Path) -> Result<()> {
    if p.as_os_str().len() > MAX_SOCKET_PATH {
        return Err(Error::Usage(format!(
            "the capture socket path is too long for the system to use ({} bytes, \
             limit {MAX_SOCKET_PATH}):\n  {}\n\n\
             Set XDG_RUNTIME_DIR, or move the store somewhere shallower.",
            p.as_os_str().len(),
            p.display()
        )));
    }
    Ok(())
}

/// A socket the overlay owns for as long as it runs.
pub struct Listener {
    inner: UnixListener,
    path: PathBuf,
}

impl Listener {
    /// Start listening, refusing to displace an overlay that is already there.
    pub fn bind(root: &Path) -> Result<Listener> {
        let p = path(root);
        check_length(&p)?;

        if p.exists() {
            // Ask before removing. A socket file proves only that a process
            // once existed; connecting is what distinguishes a live overlay
            // from the remains of a dead one.
            if UnixStream::connect(&p).is_ok() {
                return Err(Error::Usage(
                    "an overlay is already running for this store".into(),
                ));
            }
            std::fs::remove_file(&p)?;
        }

        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let inner = UnixListener::bind(&p)?;
        Ok(Listener { inner, path: p })
    }

    /// Wait for the next request to show the window.
    ///
    /// Blocks. Unrecognised input is ignored rather than refused: the channel
    /// carries one word, so anything else is a client from another version or a
    /// stray connection, and neither is worth stopping the overlay for.
    pub fn next(&self) -> Result<()> {
        loop {
            let (mut stream, _) = self.inner.accept()?;
            let mut buf = [0u8; 16];
            let n = stream.read(&mut buf).unwrap_or(0);
            if buf[..n].starts_with(b"show") {
                return Ok(());
            }
            tracing::debug!("ignoring an unrecognised message on the capture socket");
        }
    }

    /// Where it is listening, for diagnostics.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        // Tidy on the way out. Not relied upon — a killed process leaves the
        // file behind, which is why `bind` asks rather than assumes.
        std::fs::remove_file(&self.path).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("capture-sock-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn two_stores_get_two_sockets() {
        // Otherwise a second store's overlay would find the first one's socket,
        // decide somebody is home, and refuse to start.
        let a = scratch("distinct-a");
        let b = scratch("distinct-b");
        assert_ne!(path(&a), path(&b));
        std::fs::remove_dir_all(&a).ok();
        std::fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn the_same_store_always_gets_the_same_socket() {
        // The client and the overlay work it out independently; if they
        // disagreed, the chord would silently reach nothing.
        let d = scratch("stable");
        assert_eq!(path(&d), path(&d));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_deeply_nested_store_still_gets_a_usable_socket() {
        // The failure that prompted moving the socket out of the store: paths
        // are limited to about 108 bytes by the kernel, and a store can be
        // anywhere.
        let deep = scratch("deep").join("a".repeat(60)).join("b".repeat(60));
        std::fs::create_dir_all(&deep).unwrap();
        let p = path(&deep);
        assert!(
            check_length(&p).is_ok(),
            "socket path is {} bytes: {}",
            p.as_os_str().len(),
            p.display()
        );
    }

    #[test]
    fn an_impossible_path_is_refused_in_words() {
        // `path must be shorter than SUN_LEN` tells the reader nothing about
        // what to do next.
        let long = PathBuf::from("/".to_string() + &"x".repeat(200));
        let e = check_length(&long).unwrap_err();
        let said = format!("{e}");
        assert!(said.contains("too long"), "{said}");
        assert!(said.contains("XDG_RUNTIME_DIR"), "{said}");
    }

    #[test]
    fn signalling_nothing_is_not_an_error() {
        // The chord can be pressed when the overlay is not running. That is a
        // fact about the situation, not a fault, and the caller needs to be
        // able to tell it apart from a real failure.
        let d = scratch("absent");
        assert!(!signal(&d).unwrap());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_signal_reaches_a_listener() {
        let d = scratch("reaches");
        let l = Listener::bind(&d).unwrap();

        let dir = d.clone();
        let sender = std::thread::spawn(move || signal(&dir).unwrap());

        l.next().unwrap();
        assert!(sender.join().unwrap());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_second_overlay_refuses_to_start() {
        // Two overlays would both register interest in the same chord, and the
        // second would take it — leaving the first running, invisible, and
        // holding the writer lock.
        let d = scratch("second");
        let _first = Listener::bind(&d).unwrap();
        assert!(Listener::bind(&d).is_err());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_socket_left_by_a_dead_process_does_not_block_a_new_one() {
        // The file outlives its owner. Refusing to start because of it would
        // mean a crash makes the overlay unstartable until someone deletes a
        // file they have no reason to know about.
        let d = scratch("stale");
        // A path that exists with nothing listening — which is all a killed
        // process leaves behind. Leaking the listener instead would not model
        // this: the socket would still be bound and still accept connections,
        // so refusing to start would be the right answer.
        std::fs::write(path(&d), b"").unwrap();
        assert!(path(&d).exists(), "the stale file is there");
        let _fresh = Listener::bind(&d).expect("a stale socket must not block startup");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn a_live_socket_is_never_removed() {
        // The other half of the same decision. Deleting unconditionally is how
        // two overlays end up running, and it would look identical to the
        // stale-file case from the filesystem alone.
        let d = scratch("live");
        let first = Listener::bind(&d).unwrap();
        assert!(Listener::bind(&d).is_err());
        // The original is still listening, not collateral damage.
        let dir = d.clone();
        let sender = std::thread::spawn(move || signal(&dir).unwrap());
        first.next().unwrap();
        assert!(sender.join().unwrap());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn nonsense_does_not_stop_the_overlay() {
        // One word is the whole protocol, so anything else is a client from
        // another version or a stray connection. Neither is worth exiting for.
        let d = scratch("nonsense");
        let l = Listener::bind(&d).unwrap();

        let dir = d.clone();
        std::thread::spawn(move || {
            if let Ok(mut s) = UnixStream::connect(path(&dir)) {
                let _ = s.write_all(b"garbage\n");
            }
            std::thread::sleep(std::time::Duration::from_millis(30));
            let _ = signal(&dir);
        });

        // Returns for the real one, having ignored the first.
        l.next().unwrap();
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn the_socket_is_removed_on_a_clean_exit() {
        let d = scratch("clean");
        {
            let _l = Listener::bind(&d).unwrap();
            assert!(path(&d).exists());
        }
        assert!(!path(&d).exists());
        std::fs::remove_dir_all(&d).ok();
    }
}
