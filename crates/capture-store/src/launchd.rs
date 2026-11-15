//! LaunchAgents, the Mac's way to start something at login
//! (it-runs-on-a-mac D4).
//!
//! Written only when asked, as on Linux: something that arranges to start
//! itself without being told to is rightly resented. Loaded with `launchctl
//! bootstrap` into the person's GUI domain, which a managed Mac may refuse;
//! `capture doctor` asks `launchctl print` whether it took (O3).

use std::path::Path;
use std::process::{Command, Stdio};

/// The overlay's label.
pub const OVERLAY: &str = "dev.capture.overlay";

/// A LaunchAgent that runs `program` with `args` at login, once.
///
/// `KeepAlive` false: an overlay that exits on purpose, or a second copy that
/// found the socket taken, should not be restarted in a loop.
pub fn plist(label: &str, program: &str, args: &[&str]) -> String {
    let mut argv = format!("    <string>{}</string>\n", xml(program));
    for a in args {
        argv.push_str(&format!("    <string>{}</string>\n", xml(a)));
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \x20 <key>Label</key>\n  <string>{}</string>\n\
         \x20 <key>ProgramArguments</key>\n  <array>\n{argv}  </array>\n\
         \x20 <key>RunAtLoad</key>\n  <true/>\n\
         \x20 <key>KeepAlive</key>\n  <false/>\n\
         \x20 <key>ProcessType</key>\n  <string>Interactive</string>\n\
         </dict>\n</plist>\n",
        xml(label)
    )
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `gui/<uid>`, the domain a person's login items live in.
fn gui_domain() -> Option<String> {
    let out = Command::new("id").arg("-u").output().ok()?;
    let uid = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!uid.is_empty() && uid.bytes().all(|b| b.is_ascii_digit())).then(|| format!("gui/{uid}"))
}

fn launchctl(args: &[&str]) -> bool {
    Command::new("launchctl")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Load a written LaunchAgent now. False if `launchctl` refused it.
pub fn load(plist: &Path) -> bool {
    let Some(domain) = gui_domain() else {
        return false;
    };
    // Out first, so loading a changed file is not refused as already there.
    let path = plist.to_string_lossy();
    let _ = launchctl(&["bootout", &domain, &path]);
    launchctl(&["bootstrap", &domain, &path])
}

/// Unload one, before its file is removed.
pub fn unload(plist: &Path) {
    if let Some(domain) = gui_domain() {
        let _ = launchctl(&["bootout", &domain, &plist.to_string_lossy()]);
    }
}

/// Whether launchd holds `label` in this person's domain.
pub fn loaded(label: &str) -> bool {
    gui_domain().is_some_and(|d| launchctl(&["print", &format!("{d}/{label}")]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_runs_the_overlay_once_at_login() {
        let p = plist(
            OVERLAY,
            "/Users/someone/.local/bin/capture-overlay",
            &["--run"],
        );
        assert!(p.contains("<string>dev.capture.overlay</string>"));
        assert!(p.contains("<string>/Users/someone/.local/bin/capture-overlay</string>"));
        assert!(p.contains("<string>--run</string>"));
        assert!(p.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(p.contains("<key>KeepAlive</key>\n  <false/>"));
        assert!(p.starts_with("<?xml"));
    }

    #[test]
    fn a_path_cannot_break_the_xml() {
        let p = plist(OVERLAY, "/Users/a&b/<x>/capture-overlay", &[]);
        assert!(p.contains("/Users/a&amp;b/&lt;x&gt;/capture-overlay"));
        assert!(!p.contains("a&b"));
    }
}
