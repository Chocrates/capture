//! `install.sh` on a Mac, run on Linux under stand-ins (it-runs-on-a-mac 3.1).
//!
//! `uname` says Darwin, `cargo` makes stub binaries instead of building,
//! `launchctl` and every installed program record how they were called. `HOME`
//! is a scratch directory, so nothing here touches the real one.
//!
//! The claim under test is D5's and the spec's: a plain install writes no
//! LaunchAgent, so nothing starts and nothing listens until asked.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Run {
    root: PathBuf,
    out: String,
    ok: bool,
}

impl Run {
    fn home(&self) -> PathBuf {
        self.root.join("home")
    }
    /// What was run, the build left out: `cargo build -p capture-overlay`
    /// names the overlay without running it.
    fn calls(&self) -> String {
        std::fs::read_to_string(self.root.join("calls"))
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.starts_with("cargo "))
            .map(|l| format!("{l}\n"))
            .collect()
    }
    fn built(&self) -> bool {
        std::fs::read_to_string(self.root.join("calls"))
            .unwrap_or_default()
            .lines()
            .any(|l| l.starts_with("cargo "))
    }
    fn agents(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(self.home().join("Library/LaunchAgents"))
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn stub(dir: &Path, name: &str, body: &str) {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn install(name: &str, os: &str, args: &[&str]) -> Run {
    install_with(name, os, args, "exit 0")
}

fn install_with(name: &str, os: &str, args: &[&str], xcode: &str) -> Run {
    install_from(name, os, args, xcode, None)
}

/// `script` is where install.sh runs from: the repository's copy by default,
/// or one beside a download's binaries.
fn install_from(
    name: &str,
    os: &str,
    args: &[&str],
    xcode: &str,
    script: Option<fn(&Path, &Path) -> PathBuf>,
) -> Run {
    let root = std::env::temp_dir().join(format!("capture-install-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let stubs = root.join("stubs");
    let target = root.join("target");
    std::fs::create_dir_all(&stubs).unwrap();
    std::fs::create_dir_all(root.join("home")).unwrap();
    let calls = root.join("calls");
    let record = format!("echo \"$(basename \"$0\") $*\" >> '{}'", calls.display());

    stub(&stubs, "uname", &format!("echo {os}"));
    stub(&stubs, "xcode-select", xcode);
    stub(&stubs, "pkg-config", "exit 1");
    stub(&stubs, "launchctl", &format!("{record}\nexit 0"));
    stub(&stubs, "systemctl", &format!("{record}\nexit 0"));
    stub(&stubs, "sleep", "exit 0");
    stub(&stubs, "xattr", &format!("{record}\nexit 0"));
    // Each program it installs records how it was called and succeeds.
    stub(&stubs, "program", &format!("{record}\nexit 0"));
    stub(
        &stubs,
        "cargo",
        &format!(
            "{record}\nmkdir -p '{r}'\nfor b in capture capture-overlay capture-syncd capture-desktop capture-alerts; do\n  \
             cp '{p}' '{r}'/$b\ndone\nexit 0",
            r = target.join("release").display(),
            p = stubs.join("program").display(),
        ),
    );

    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../install.sh");
    let path = match script {
        Some(make) => make(&root, &stubs.join("program")),
        None => repo,
    };
    let out = Command::new("sh")
        .arg(path)
        .args(args)
        .current_dir(&root)
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", stubs.display()))
        .env("HOME", root.join("home"))
        .env("SHELL", "/bin/zsh")
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .expect("sh runs");
    Run {
        out: format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
        ok: out.status.success(),
        root,
    }
}

#[test]
fn a_plain_mac_install_starts_nothing() {
    let r = install("plain", "Darwin", &[]);
    assert!(r.ok, "{}", r.out);
    assert!(r.agents().is_empty(), "wrote {:?}", r.agents());
    let calls = r.calls();
    assert!(!calls.contains("launchctl"), "{calls}");
    assert!(!calls.contains("systemctl"), "{calls}");
    assert!(!calls.contains("capture-overlay"), "{calls}");
    assert!(!calls.contains("capture-syncd"), "{calls}");
    assert!(calls.contains("capture init"), "{calls}");
    // The desktop app is always built on a Mac: WKWebView is always there.
    assert!(r.home().join(".local/bin/capture-desktop").exists());
    // Built and bundled from source too, and put where apps go.
    assert!(
        r.home()
            .join("Applications/Capture Alerts.app/Contents/Info.plist")
            .exists()
    );
    assert!(r.out.contains("~/.zshrc"), "{}", r.out);
    assert!(
        r.out.contains("nothing listens on the network"),
        "{}",
        r.out
    );
}

#[test]
fn with_sync_a_mac_gets_a_launchagent_for_the_daemon() {
    let r = install("sync", "Darwin", &["--with-sync"]);
    assert!(r.ok, "{}", r.out);
    assert_eq!(r.agents(), vec!["dev.capture.syncd.plist".to_string()]);
    let plist = std::fs::read_to_string(
        r.home()
            .join("Library/LaunchAgents/dev.capture.syncd.plist"),
    )
    .unwrap();
    assert!(plist.contains("<string>dev.capture.syncd</string>"));
    assert!(plist.contains(&format!(
        "<string>{}/.local/bin/capture-syncd</string>",
        r.home().display()
    )));
    assert!(plist.contains("<key>CAPTURE_HOME</key>"));
    assert!(
        r.calls().contains("launchctl bootstrap gui/"),
        "{}",
        r.calls()
    );
    assert!(!r.calls().contains("systemctl"), "{}", r.calls());
}

#[test]
fn with_chord_a_mac_asks_the_overlay_to_start_at_login() {
    let r = install("chord", "Darwin", &["--with-chord"]);
    assert!(r.ok, "{}", r.out);
    // The overlay writes its own LaunchAgent (and is tested doing so); the
    // script's part is asking it to.
    assert!(
        r.calls().contains("capture-overlay --autostart"),
        "{}",
        r.calls()
    );
    assert!(
        r.calls().contains("capture-overlay --register"),
        "{}",
        r.calls()
    );
}

#[test]
fn with_export_a_mac_gets_a_daily_agent() {
    let r = install("export", "Darwin", &["--with-export"]);
    assert!(r.ok, "{}", r.out);
    assert_eq!(r.agents(), vec!["dev.capture.export.plist".to_string()]);
    let plist = std::fs::read_to_string(
        r.home()
            .join("Library/LaunchAgents/dev.capture.export.plist"),
    )
    .unwrap();
    assert!(plist.contains("<string>export</string>"));
    assert!(plist.contains("StartCalendarInterval"));
}

#[test]
fn a_mac_without_apples_tools_is_told_how_to_get_them() {
    let r = install_with("no-xcode", "Darwin", &[], "exit 2");
    assert!(!r.ok);
    assert!(r.out.contains("xcode-select --install"), "{}", r.out);
    assert!(!r.calls().contains("capture init"), "installed anyway");
}

#[test]
fn linux_writes_no_launchagents() {
    let r = install("linux", "Linux", &["--with-sync"]);
    assert!(r.ok, "{}", r.out);
    assert!(r.agents().is_empty());
    assert!(r.calls().contains("systemctl"), "{}", r.calls());
    assert!(!r.calls().contains("launchctl"), "{}", r.calls());
}

/// The download CI makes: install.sh beside built binaries and the app, no
/// Cargo.toml. Its Mac may have no Rust and no Xcode tools.
fn download(root: &Path, program: &Path) -> PathBuf {
    let d = root.join("capture-macos-arm64");
    std::fs::create_dir_all(d.join("Capture Alerts.app/Contents/MacOS")).unwrap();
    for b in [
        "capture",
        "capture-overlay",
        "capture-syncd",
        "capture-desktop",
    ] {
        std::fs::copy(program, d.join(b)).unwrap();
    }
    std::fs::copy(
        program,
        d.join("Capture Alerts.app/Contents/MacOS/capture-alerts"),
    )
    .unwrap();
    let script = d.join("install.sh");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../install.sh"),
        &script,
    )
    .unwrap();
    script
}

#[test]
fn a_download_installs_what_it_holds_and_builds_nothing() {
    // No Xcode tools: a build would stop at the first check.
    let r = install_from(
        "download",
        "Darwin",
        &["--with-chord"],
        "exit 2",
        Some(download),
    );
    assert!(r.ok, "{}", r.out);
    let calls = r.calls();
    assert!(!r.built(), "built anyway");
    assert!(calls.contains("xattr -dr com.apple.quarantine"), "{calls}");
    for b in [
        "capture",
        "capture-overlay",
        "capture-syncd",
        "capture-desktop",
    ] {
        assert!(r.home().join(".local/bin").join(b).exists(), "{b}");
    }
    assert!(
        r.home()
            .join("Applications/Capture Alerts.app/Contents/MacOS/capture-alerts")
            .exists()
    );
    assert!(calls.contains("capture init"), "{calls}");
    assert!(calls.contains("capture-overlay --autostart"), "{calls}");
}
