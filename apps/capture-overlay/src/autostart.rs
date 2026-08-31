//! Being there before the chord is pressed.
//!
//! # Why the overlay has to already be running
//!
//! The desktop's own overview is the model. Pressing Super does not launch
//! anything — `gnome-shell` is already running and simply reveals a surface it
//! built long ago. That is the entire reason it feels instant, and the reason
//! nothing else on the desktop feels quite the same.
//!
//! Starting a window on a keypress means paying for process start, dynamic
//! linking, a GPU context and font rasterisation before the first pixel. On
//! this machine that is a second or more; the resident path measures under two
//! milliseconds. A capture gesture that takes a visible moment to appear is one
//! people stop trusting, and then stop using.
//!
//! So the process starts at login and waits. What it costs while waiting is
//! about thirty megabytes of private memory and, with the wake path in
//! `window.rs`, no measurable processor time.
//!
//! # Why this writes a file rather than a systemd unit
//!
//! A `.desktop` entry in the autostart directory is understood by every desktop
//! environment, needs no daemon manager, and is a single file the user can read
//! and delete. A systemd user unit would work on this machine and not on a
//! system without systemd, and it puts the arrangement somewhere less obvious
//! than a directory named `autostart`.
//!
//! The file is written only when asked. Something that quietly arranges to
//! start itself at every login, without being told to, is a thing people
//! rightly resent.

use std::path::PathBuf;

use capture_store::{Error, Result};

/// Where desktops look for things to start at login.
fn autostart_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("autostart"))
}

fn entry_path() -> Option<PathBuf> {
    Some(autostart_dir()?.join("capture-overlay.desktop"))
}

/// The entry's contents.
///
/// `NoDisplay` keeps it out of the applications menu: it is not something to
/// launch, it is something that should already be running. Launching a second
/// copy would find the socket taken and refuse, which is correct but is a
/// confusing thing to have offered someone.
fn entry(exe: &str) -> String {
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=capture\n\
         Comment=Holds the capture window ready for the chord\n\
         Exec={exe} --run\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n\
         Terminal=false\n"
    )
}

/// Arrange for the overlay to start at login.
pub fn install() -> Result<PathBuf> {
    let path = entry_path().ok_or_else(|| Error::Usage("cannot find a config directory".into()))?;
    let exe = std::env::current_exe()
        .map_err(|e| Error::Usage(format!("cannot find this executable: {e}")))?;

    // The absolute path, deliberately. A bare name would depend on whatever
    // PATH the session manager happens to have, which is frequently not the
    // one a login shell builds — and the failure is silent: the entry exists,
    // nothing starts, and the chord does nothing.
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, entry(&exe.to_string_lossy()))?;
    Ok(path)
}

/// Stop it starting at login.
pub fn remove() -> Result<bool> {
    let path = entry_path().ok_or_else(|| Error::Usage("cannot find a config directory".into()))?;
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(&path)?;
    Ok(true)
}

/// Whether it is arranged to start at login.
pub fn installed() -> bool {
    entry_path().is_some_and(|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_runs_the_overlay_rather_than_the_client() {
        // `--show` would exit immediately, leaving nothing listening — and the
        // chord would then spawn a client that reaches nothing, which is the
        // registered-but-dead state the gesture check exists to catch.
        let e = entry("/usr/bin/capture-overlay");
        assert!(e.contains("Exec=/usr/bin/capture-overlay --run"), "{e}");
        assert!(!e.contains("--show"));
    }

    #[test]
    fn the_entry_uses_an_absolute_path() {
        // A session manager's PATH is frequently not the one a login shell
        // builds, and the failure is silent: the entry exists, nothing starts,
        // and pressing the chord does nothing at all.
        let e = entry("/home/someone/.local/bin/capture-overlay");
        assert!(e.contains("Exec=/home/someone/.local/bin/capture-overlay"));
    }

    #[test]
    fn it_does_not_offer_itself_as_something_to_launch() {
        // It should already be running. A menu entry would invite starting a
        // second copy, which finds the socket taken and refuses — correct, and
        // a confusing thing to have offered.
        assert!(entry("/usr/bin/capture-overlay").contains("NoDisplay=true"));
    }

    #[test]
    fn the_entry_is_a_complete_desktop_file() {
        let e = entry("/usr/bin/capture-overlay");
        assert!(e.starts_with("[Desktop Entry]"));
        for key in ["Type=Application", "Name=", "Terminal=false"] {
            assert!(e.contains(key), "missing {key}");
        }
    }
}
