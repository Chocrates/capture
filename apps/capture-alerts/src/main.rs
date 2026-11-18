//! `Capture Alerts.app`: capture's own name in Notification Center
//! (it-runs-on-a-mac D9).
//!
//! For now only the probe, which answers one question on the person's Mac: does
//! macOS, and whoever manages that Mac, let a self-signed app post alerts at
//! all? Nothing else is worth building until that is known, and only that
//! machine can say.
//!
//! ```text
//! open ~/Applications/Capture\ Alerts.app          first launch: asks permission
//! ~/Applications/Capture\ Alerts.app/Contents/MacOS/capture-alerts --probe
//! ~/Applications/Capture\ Alerts.app/Contents/MacOS/capture-alerts --settings
//! ```
//!
//! Every line is `CODE pass|fail|info: reason` with a fixed reason, the way
//! `capture doctor` prints (D6): the only thing ever printed that was not
//! written here is an Apple error number.

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match args.first().map(String::as_str) {
        None => Mode::Opened,
        Some("--probe") => Mode::Probe,
        Some("--settings") => Mode::Settings,
        Some(_) => {
            eprintln!("usage: capture-alerts [--probe | --settings]");
            return std::process::ExitCode::from(2);
        }
    };
    run(mode)
}

/// What the probe does and where it says it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code, reason = "off a Mac only the refusal is reached")]
enum Mode {
    /// Launched from Finder or `open`: the launch macOS registers the app on.
    /// Asks, posts, and writes the lines to a file, since there is no terminal.
    Opened,
    /// From a terminal: all of it, ending with "did you see it?".
    Probe,
    /// Reads the settings and asks nothing. Safe anywhere, CI included.
    Settings,
}

#[cfg(not(target_os = "macos"))]
fn run(_mode: Mode) -> std::process::ExitCode {
    eprintln!("capture-alerts is the Mac's alert app; there is nothing to do here.");
    std::process::ExitCode::from(2)
}

#[cfg(target_os = "macos")]
fn run(mode: Mode) -> std::process::ExitCode {
    let lines = mac::probe(mode);
    let failed = lines.iter().any(|l| l.contains(" fail: "));
    if mode == Mode::Opened {
        mac::leave(&lines);
    } else {
        for l in &lines {
            println!("{l}");
        }
    }
    if failed {
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::Mode;
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_user_notifications::{
        UNAlertStyle, UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent,
        UNNotification, UNNotificationRequest, UNNotificationSetting, UNNotificationSettings,
        UNNotificationSound, UNUserNotificationCenter,
    };
    use std::ptr::NonNull;
    use std::sync::mpsc;
    use std::time::Duration;

    /// The identifier the test alert is posted under, so the probe can find
    /// it among the delivered ones and take it away again.
    const PROBE_ID: &str = "dev.capture.alerts.probe";

    /// How long to wait for any one answer from the notification daemon. The
    /// permission prompt is a person reading a dialog, so it gets longer.
    const ANSWER: Duration = Duration::from_secs(10);
    const PROMPT: Duration = Duration::from_secs(120);

    pub fn probe(mode: Mode) -> Vec<String> {
        let mut out = Vec::new();

        // Without a bundle the notification center raises an Objective-C
        // exception and the process aborts with nothing said. Checked first.
        let inside = NSBundle::mainBundle()
            .bundleIdentifier()
            .is_some_and(|id| id.to_string() == "dev.capture.alerts");
        if !inside {
            out.push("A1 fail: not inside Capture Alerts.app".into());
            return out;
        }
        out.push("A1 pass: inside the app".into());

        let center = UNUserNotificationCenter::currentNotificationCenter();

        let before = settings(&center);
        out.push(match before {
            Some(s) => format!("A2 info: before: {}", status(s.0)),
            None => "A2 fail: no answer about settings".into(),
        });

        if mode != Mode::Settings {
            out.push(ask(&center));
            out.push(post(&center));
            // Delivered is the daemon's word that it took it. Not the same as
            // seen, which only the person can say.
            std::thread::sleep(Duration::from_secs(2));
            out.push(listed(&center));
        }

        out.push(match settings(&center) {
            Some(s) => format!(
                "A6 info: now: {}, style {}, alerts {}, sound {}",
                status(s.0),
                style(s.1),
                setting(s.2),
                setting(s.3)
            ),
            None => "A6 fail: no answer about settings".into(),
        });

        if mode == Mode::Probe {
            out.push(seen());
            center.removeAllDeliveredNotifications();
        }
        out
    }

    /// Settings, as the four values the lines need.
    type Settings = (
        UNAuthorizationStatus,
        UNAlertStyle,
        UNNotificationSetting,
        UNNotificationSetting,
    );

    fn settings(center: &UNUserNotificationCenter) -> Option<Settings> {
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |s: NonNull<UNNotificationSettings>| {
            // SAFETY: the daemon hands over a live settings object for the
            // duration of the call.
            let s = unsafe { s.as_ref() };
            let _ = tx.send((
                s.authorizationStatus(),
                s.alertStyle(),
                s.alertSetting(),
                s.soundSetting(),
            ));
        });
        center.getNotificationSettingsWithCompletionHandler(&block);
        rx.recv_timeout(ANSWER).ok()
    }

    fn ask(center: &UNUserNotificationCenter) -> String {
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |granted: Bool, err: *mut NSError| {
            // SAFETY: null or a live error for the duration of the call.
            let code = unsafe { err.as_ref() }.map(error_code);
            let _ = tx.send((granted.as_bool(), code));
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
            &block,
        );
        match rx.recv_timeout(PROMPT) {
            Ok((true, _)) => "A3 pass: allowed".into(),
            Ok((false, None)) => "A3 fail: refused".into(),
            Ok((false, Some(c))) => format!("A3 fail: refused, error {c}"),
            Err(_) => "A3 fail: no answer".into(),
        }
    }

    fn post(center: &UNUserNotificationCenter) -> String {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str("capture"));
        content.setBody(&NSString::from_str(
            "Test alert. If you can read this, it works.",
        ));
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(PROBE_ID),
            &content,
            None,
        );
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |err: *mut NSError| {
            // SAFETY: null or a live error for the duration of the call.
            let _ = tx.send(unsafe { err.as_ref() }.map(error_code));
        });
        center.addNotificationRequest_withCompletionHandler(&request, Some(&block));
        match rx.recv_timeout(ANSWER) {
            Ok(None) => "A4 pass: posted".into(),
            Ok(Some(c)) => format!("A4 fail: error {c}"),
            Err(_) => "A4 fail: no answer".into(),
        }
    }

    fn listed(center: &UNUserNotificationCenter) -> String {
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(
            move |all: NonNull<objc2_foundation::NSArray<UNNotification>>| {
                // SAFETY: a live array for the duration of the call.
                let all = unsafe { all.as_ref() };
                let ours = all
                    .iter()
                    .any(|n| n.request().identifier().to_string() == PROBE_ID);
                let _ = tx.send(ours);
            },
        );
        center.getDeliveredNotificationsWithCompletionHandler(&block);
        match rx.recv_timeout(ANSWER) {
            Ok(true) => "A5 pass: in Notification Center".into(),
            Ok(false) => "A5 fail: not in Notification Center".into(),
            Err(_) => "A5 fail: no answer".into(),
        }
    }

    fn seen() -> String {
        use std::io::{BufRead, Write};
        print!("Did a notification saying \"capture — Test alert\" appear? [y/n] ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
        match line.trim() {
            "y" | "Y" | "yes" => "A7 pass: seen".into(),
            _ => "A7 fail: not seen".into(),
        }
    }

    /// An Apple error as a number and which family it is from. The domain is
    /// named only when it is one of Apple's own, so nothing free-form is
    /// printed.
    fn error_code(e: &NSError) -> String {
        let family = match e.domain().to_string().as_str() {
            "UNErrorDomain" => "un",
            "NSCocoaErrorDomain" => "cocoa",
            "NSOSStatusErrorDomain" => "os",
            _ => "other",
        };
        format!("{family} {}", e.code())
    }

    fn status(s: UNAuthorizationStatus) -> &'static str {
        match s {
            UNAuthorizationStatus::NotDetermined => "not asked yet",
            UNAuthorizationStatus::Denied => "denied",
            UNAuthorizationStatus::Authorized => "allowed",
            UNAuthorizationStatus::Provisional => "provisional",
            _ => "unknown",
        }
    }

    fn style(s: UNAlertStyle) -> &'static str {
        match s {
            UNAlertStyle::Alert => "alerts",
            UNAlertStyle::Banner => "banners",
            UNAlertStyle::None => "none",
            _ => "unknown",
        }
    }

    fn setting(s: UNNotificationSetting) -> &'static str {
        match s {
            UNNotificationSetting::Enabled => "on",
            UNNotificationSetting::Disabled => "off",
            UNNotificationSetting::NotSupported => "unsupported",
            _ => "unknown",
        }
    }

    /// Launched without a terminal: leave the lines where the person can read
    /// them with `cat`, on their own machine.
    pub fn leave(lines: &[String]) {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let dir = std::path::Path::new(&home).join("Library/Logs");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("capture-alerts.txt"), lines.join("\n") + "\n");
    }
}
