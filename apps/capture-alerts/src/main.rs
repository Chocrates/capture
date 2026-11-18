//! `Capture Alerts.app`: capture's own name in Notification Center
//! (it-runs-on-a-mac D9).
//!
//! `--say <title> <body>` posts one alert for the overlay, and says by its
//! exit code whether Notification Center holds it.
//!
//! The probe answers what only the person's Mac can: does macOS, and whoever
//! manages that Mac, let a self-signed app post alerts, and are they seen?
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
        Some("--later") => Mode::Later,
        Some("--say") if args.len() == 3 => return say(&args[1], &args[2]),
        Some(_) => {
            eprintln!(
                "usage: capture-alerts [--probe | --later | --settings | --say <title> <body>]"
            );
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
    /// Posts the test alert for five seconds from now and exits at once, so
    /// it arrives while the app is not running and cannot count as the one
    /// in front. How a real alert, scheduled ahead, would arrive.
    Later,
}

#[cfg(target_os = "macos")]
fn say(title: &str, body: &str) -> std::process::ExitCode {
    mac::say(title, body)
}

#[cfg(not(target_os = "macos"))]
fn say(_title: &str, _body: &str) -> std::process::ExitCode {
    std::process::ExitCode::from(2)
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
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, ProtocolObject};
    use objc2::{AllocAnyThread, define_class, msg_send};
    use objc2_foundation::{NSBundle, NSError, NSObject, NSObjectProtocol, NSString};
    use objc2_user_notifications::{
        UNAlertStyle, UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent,
        UNNotification, UNNotificationPresentationOptions, UNNotificationRequest,
        UNNotificationSetting, UNNotificationSettings, UNNotificationSound,
        UNTimeIntervalNotificationTrigger, UNUserNotificationCenter,
        UNUserNotificationCenterDelegate,
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

    define_class!(
        /// Asked by macOS what to do with an alert that arrives while this
        /// app is the one in front, which is what a process started from a
        /// terminal counts as. With no answer the banner is not shown: filed
        /// in Notification Center and nothing on screen (first probe on the
        /// work Mac: A5 pass, A7 fail).
        #[unsafe(super(NSObject))]
        #[name = "CaptureAlertsShowAnyway"]
        struct ShowAnyway;

        unsafe impl NSObjectProtocol for ShowAnyway {}

        unsafe impl UNUserNotificationCenterDelegate for ShowAnyway {
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                handler.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List
                    | UNNotificationPresentationOptions::Sound,));
            }
        }
    );

    impl ShowAnyway {
        fn new() -> Retained<Self> {
            let this = Self::alloc().set_ivars(());
            // SAFETY: NSObject's init, on an object just allocated.
            unsafe { msg_send![super(this), init] }
        }
    }

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
        // The delegate is a weak property, so it is held here for the run.
        let shows = ShowAnyway::new();
        center.setDelegate(Some(ProtocolObject::from_ref(&*shows)));

        let before = settings(&center);
        out.push(match before {
            Some(s) => format!("A2 info: before: {}", status(s.0)),
            None => "A2 fail: no answer about settings".into(),
        });

        if mode == Mode::Later {
            out.push(match post(&center, Some(5.0)) {
                l if l.contains(" pass: ") => "A8 pass: posted for 5 seconds from now".into(),
                l => l.replacen("A4", "A8", 1),
            });
            return out;
        }

        if mode != Mode::Settings {
            out.push(ask(&center));
            out.push(post(&center, None));
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

    fn post(center: &UNUserNotificationCenter, after: Option<f64>) -> String {
        match add(
            center,
            PROBE_ID,
            "capture",
            "Test alert. If you can read this, it works.",
            after,
        ) {
            Some(None) => "A4 pass: posted".into(),
            Some(Some(c)) => format!("A4 fail: error {c}"),
            None => "A4 fail: no answer".into(),
        }
    }

    fn listed(center: &UNUserNotificationCenter) -> String {
        match delivered(center, PROBE_ID) {
            Some(true) => "A5 pass: in Notification Center".into(),
            Some(false) => "A5 fail: not in Notification Center".into(),
            None => "A5 fail: no answer".into(),
        }
    }

    /// Post one alert. `None` if the daemon never answered, `Some(None)` if
    /// it took it, `Some(Some(code))` if it refused.
    fn add(
        center: &UNUserNotificationCenter,
        id: &str,
        title: &str,
        body: &str,
        after: Option<f64>,
    ) -> Option<Option<String>> {
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        let trigger = after
            .map(|s| UNTimeIntervalNotificationTrigger::triggerWithTimeInterval_repeats(s, false));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(id),
            &content,
            trigger.as_deref().map(|t| &**t),
        );
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |err: *mut NSError| {
            // SAFETY: null or a live error for the duration of the call.
            let _ = tx.send(unsafe { err.as_ref() }.map(error_code));
        });
        center.addNotificationRequest_withCompletionHandler(&request, Some(&block));
        rx.recv_timeout(ANSWER).ok()
    }

    /// Whether Notification Center holds the alert posted as `id`.
    fn delivered(center: &UNUserNotificationCenter, id: &str) -> Option<bool> {
        let id = id.to_string();
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(
            move |all: NonNull<objc2_foundation::NSArray<UNNotification>>| {
                // SAFETY: a live array for the duration of the call.
                let all = unsafe { all.as_ref() };
                let ours = all
                    .iter()
                    .any(|n| n.request().identifier().to_string() == id);
                let _ = tx.send(ours);
            },
        );
        center.getDeliveredNotificationsWithCompletionHandler(&block);
        rx.recv_timeout(ANSWER).ok()
    }

    /// One real alert, for `capture_store::notifier::notify`. Prints nothing:
    /// the exit code is the answer. 0 when Notification Center holds it, 3
    /// when this app is not allowed to post, 1 for anything else. Only 0
    /// counts as said, so anything else is tried again later.
    pub fn say(title: &str, body: &str) -> std::process::ExitCode {
        use std::process::ExitCode;
        let inside = NSBundle::mainBundle()
            .bundleIdentifier()
            .is_some_and(|id| id.to_string() == "dev.capture.alerts");
        if !inside {
            return ExitCode::FAILURE;
        }
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let shows = ShowAnyway::new();
        center.setDelegate(Some(ProtocolObject::from_ref(&*shows)));
        match settings(&center) {
            Some((UNAuthorizationStatus::Authorized | UNAuthorizationStatus::Provisional, ..)) => {}
            Some(_) => return ExitCode::from(3),
            None => return ExitCode::FAILURE,
        }
        // One process an alert, so the process id tells two alerts apart and
        // one does not replace another.
        let id = format!("dev.capture.alerts.{}", std::process::id());
        if add(&center, &id, title, body, None) != Some(None) {
            return ExitCode::FAILURE;
        }
        // Shown is not knowable; held is. Asked twice, since the daemon
        // files it a moment after taking it.
        for _ in 0..2 {
            std::thread::sleep(Duration::from_millis(500));
            if delivered(&center, &id) == Some(true) {
                return ExitCode::SUCCESS;
            }
        }
        ExitCode::FAILURE
    }

    fn seen() -> String {
        use std::io::{BufRead, Write};
        print!(
            "Did a banner saying \"capture — Test alert\" appear on screen? \
             (Not just in Notification Center.) [y/n] "
        );
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
