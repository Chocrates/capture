//! `capture doctor`: every check, as codes a person can read aloud.
//!
//! # Why codes
//!
//! Made for a machine nothing can be copied off (it-runs-on-a-mac D6). The
//! person reads the lines back, so each one has to be short, fixed, and safe
//! to say: a code, a result, a reason. The reason is a `&'static str` by type,
//! so nothing the system or the archive supplied — note text, a path, a user
//! or host name, an address, a key — can reach a line. `tests/doctor.rs` seeds
//! all of those and checks none appears. The codes are in
//! `docs/doctor-codes.md`, and a test checks every line is there.
//!
//! # What it does to the machine
//!
//! Takes and releases the writer lock, connects to the overlay's socket
//! without sending anything, and sends one notification (none under
//! `CAPTURE_SILENT`). `--ask` then asks whether it appeared, on the terminal.

use std::io::{BufRead, Write};
use std::path::Path;

use capture_store::{Result, StorePaths, lock::Writer, log::Log, ops::Ops, store};

/// How long since the chord last worked before H3 fails.
const CHORD_SEEN_MS: u64 = 14 * 24 * 60 * 60 * 1000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Pass,
    Fail,
    Skip,
}

struct Line {
    code: &'static str,
    outcome: Outcome,
    reason: &'static str,
}

fn line(code: &'static str, outcome: Outcome, reason: &'static str) -> Line {
    Line {
        code,
        outcome,
        reason,
    }
}

use Outcome::{Fail, Pass, Skip};

/// Run every check and print one line for each.
///
/// Always succeeds as a command: a failing check is an answer, not an error,
/// and a doctor that stopped at the first failure would hide the rest.
pub fn run(paths: &StorePaths, ask: bool) -> Result<()> {
    let mut lines = Vec::new();
    store_checks(paths, &mut lines);
    chord_checks(paths, &mut lines);
    overlay_checks(paths, &mut lines);
    notifier_checks(ask, &mut lines);
    lines.push(match beside_or_on_path("capture-desktop") {
        true => line("D1", Pass, "present, not launched"),
        false => line("D1", Fail, "not installed"),
    });
    sync_checks(paths, &mut lines);

    for l in &lines {
        let result = match l.outcome {
            Pass => "pass",
            Fail => "fail",
            Skip => "skip",
        };
        say!("{} {result}: {}", l.code, l.reason);
    }
    Ok(())
}

fn store_checks(paths: &StorePaths, out: &mut Vec<Line>) {
    if !store::exists(paths) {
        out.push(line("B1", Fail, "no store here"));
        out.push(line("B2", Skip, "no store"));
        out.push(line("B3", Skip, "no store"));
        return;
    }
    let opens = Log::open_read(&paths.events).is_ok()
        && capture_store::projection::current(paths).is_ok()
        && capture_store::identity::Identity::load(paths).is_ok();
    out.push(if opens {
        line("B1", Pass, "store opens")
    } else {
        line("B1", Fail, "store does not open")
    });

    out.push(
        match Writer::acquire(&paths.root.join("writer.lock"), "doctor") {
            Ok(Some(held)) => {
                drop(held);
                line("B2", Pass, "taken and released")
            }
            Ok(None) => line("B2", Fail, "held by another process"),
            Err(_) => line("B2", Fail, "cannot take the lock"),
        },
    );

    out.push(match crate::verify::faults(paths) {
        Ok(0) => line("B3", Pass, "every event checks out"),
        Ok(_) => line("B3", Fail, "damaged events, run capture verify"),
        Err(_) => line("B3", Fail, "log cannot be read"),
    });
}

fn chord_checks(paths: &StorePaths, out: &mut Vec<Line>) {
    let mechanism = capture_store::hotkey::select(&capture_store::hotkey::Host);
    if mechanism == capture_store::hotkey::Mechanism::None {
        out.push(line("H1", Fail, "no mechanism on this desktop"));
        out.push(line("H2", Skip, "no mechanism"));
        out.push(line("H3", Skip, "no mechanism"));
        return;
    }
    use capture_store::hotkey::Mechanism;
    out.push(line(
        "H1",
        Pass,
        match mechanism {
            Mechanism::Portal => "desktop portal",
            Mechanism::Gnome => "gnome keybinding",
            Mechanism::X11 => "x11 grab",
            Mechanism::MacCarbon => "carbon hotkey",
            Mechanism::None => "none",
        },
    ));

    let ops = Ops::open_read(&paths.ops).ok();
    out.push(
        match ops
            .as_ref()
            .and_then(|o| o.get(capture_store::hotkey::REGISTRATION).ok().flatten())
            .as_deref()
        {
            Some("ok") => line("H2", Pass, "registered"),
            Some(_) => line("H2", Fail, "registration failed"),
            None => line("H2", Skip, "never registered here"),
        },
    );

    let now = crate::read::now_ms() as u64;
    out.push(
        match ops
            .as_ref()
            .and_then(|o| o.gesture_last_fired().ok().flatten())
        {
            Some(at) if now.saturating_sub(at) <= CHORD_SEEN_MS => {
                line("H3", Pass, "seen within 14 days")
            }
            Some(_) => line("H3", Fail, "not seen for 14 days"),
            None => line("H3", Fail, "never seen"),
        },
    );
}

fn overlay_checks(paths: &StorePaths, out: &mut Vec<Line>) {
    let running = std::process::Command::new("pgrep")
        .args(["-x", "capture-overlay"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    out.push(match running {
        Ok(s) if s.success() => line("O1", Pass, "running"),
        Ok(_) => line("O1", Fail, "not running"),
        Err(_) => line("O1", Skip, "cannot list processes"),
    });

    // Connected and dropped: the overlay reads an empty message and ignores
    // it. Sending `show` would put a window in front of the person.
    let socket = capture_store::paths::overlay_socket(&paths.root);
    out.push(match std::os::unix::net::UnixStream::connect(&socket) {
        Ok(_) => line("O2", Pass, "socket answers"),
        Err(_) if socket.exists() => line("O2", Fail, "socket left behind, nothing answers"),
        Err(_) => line("O2", Fail, "no socket"),
    });

    out.push(
        match capture_store::paths::overlay_login_entry().is_some_and(|p| p.exists()) {
            false => line("O3", Fail, "does not start at login"),
            true if cfg!(target_os = "macos")
                && !capture_store::launchd::loaded(capture_store::launchd::OVERLAY) =>
            {
                line("O3", Fail, "login item written, launchd refused it")
            }
            true => line("O3", Pass, "starts at login"),
        },
    );
}

fn notifier_checks(ask: bool, out: &mut Vec<Line>) {
    let present = on_path(capture_store::notifier::NOTIFIER);
    out.push(if present {
        line("N1", Pass, "notifier present")
    } else {
        line("N1", Fail, "notifier missing")
    });

    let sent = if !present {
        out.push(line("N2", Skip, "no notifier"));
        false
    } else if std::env::var_os("CAPTURE_SILENT").is_some() {
        out.push(line("N2", Skip, "silenced"));
        false
    } else if capture_store::notifier::notify(
        "capture doctor",
        "If you can see this, notifications reach you.",
    ) {
        out.push(line("N2", Pass, "sent"));
        true
    } else {
        out.push(line("N2", Fail, "refused"));
        false
    };

    out.push(if !ask {
        line("N3", Skip, "not asked")
    } else if !sent {
        line("N3", Skip, "nothing was sent")
    } else {
        // On stderr, so stdout stays the lines and nothing else.
        eprint!("Did a notification from \"capture doctor\" appear? [y/n] ");
        let _ = std::io::stderr().flush();
        let mut answer = String::new();
        let _ = std::io::stdin().lock().read_line(&mut answer);
        if answer.trim().eq_ignore_ascii_case("y") || answer.trim().eq_ignore_ascii_case("yes") {
            line("N3", Pass, "seen")
        } else {
            line("N3", Fail, "not seen")
        }
    });
}

fn sync_checks(paths: &StorePaths, out: &mut Vec<Line>) {
    if !beside_or_on_path("capture-syncd") {
        out.push(line("S1", Skip, "not installed"));
        out.push(line("S2", Skip, "sync not installed"));
        out.push(line("S3", Skip, "sync not installed"));
        return;
    }
    out.push(line("S1", Pass, "installed"));

    let Ok(ops) = Ops::open_read(&paths.ops) else {
        out.push(line("S2", Skip, "no store"));
        out.push(line("S3", Skip, "no store"));
        return;
    };
    let paired = crate::ops::paired_others(paths).unwrap_or(0);
    let addressed = ops.peer_addresses().map(|p| p.len()).unwrap_or(0);
    out.push(if paired > 0 || addressed > 0 {
        line("S2", Pass, "a peer is configured")
    } else {
        line("S2", Fail, "no peer")
    });

    use capture_store::syncstate::SyncState;
    let now = crate::read::now_ms() as u64;
    out.push(
        match capture_store::syncstate::sync_state(&ops, paired, now) {
            Ok(SyncState::Alone) => line("S3", Skip, "nothing paired"),
            Ok(SyncState::NeverRan) => line("S3", Fail, "no round has run"),
            Ok(SyncState::NotRunning { .. }) => line("S3", Fail, "rounds stopped"),
            Ok(SyncState::NeverReachedAnyone) => line("S3", Fail, "never reached a peer"),
            Ok(SyncState::ReachingNobody { .. }) => line("S3", Fail, "reaching nobody"),
            Ok(SyncState::Working) => line("S3", Pass, "reaching a peer"),
            Err(_) => line("S3", Fail, "sync state cannot be read"),
        },
    );
}

/// A program installed beside this one, as `make install` puts them, or on
/// `PATH`.
fn beside_or_on_path(name: &str) -> bool {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join(name)))
        .is_some_and(|p| is_program(&p));
    beside || on_path(name)
}

fn on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|p| std::env::split_paths(&p).any(|d| is_program(&d.join(name))))
}

fn is_program(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
