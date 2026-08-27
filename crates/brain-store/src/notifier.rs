//! Which device says something, and what it says.
//!
//! # A default is a rule; a setting is a decision
//!
//! §2.4.1 makes this the whole of the design here, and §8.9 makes it a rule the
//! system may not break: **a default is never written to the log on the user's
//! behalf**.
//!
//! An earlier draft in the design had the first paired device emit a `setting`
//! choosing itself as notifier. That is a machine-authored preference recorded
//! as a user decision — `brain status` would then report it as "set by you", it
//! could never be centrally improved because changing the default would not
//! reach anybody, and there is no way to tell it apart from a choice somebody
//! actually made.
//!
//! So: until the user chooses, the notifier is decided by a rule evaluated
//! locally and written nowhere. [`chosen`] returns `None` in that case, and it
//! is `None` that `brain status` reports as a default.
//!
//! # Which cues can be fired here, and which cannot
//!
//! Declaring a cue type requires being able to observe it. A device that
//! claimed `app_focus` and could not see it would produce an intent that is
//! registered, reported as working, and silently never fires — the same failure
//! shape as a dead chord.
//!
//! On this target — GNOME on Wayland — the foreground application is not
//! observable at all: there is no protocol for it, `Shell.Eval` is disabled,
//! and the Xwayland `_NET_ACTIVE_WINDOW` answer is a plausible-looking zero.
//! M1 established that for the ambient envelope (§2.3) and it is the same fact
//! here, so `app_focus` is refused rather than accepted and dropped.

use crate::projection::Store;
use crate::{Result, StorePaths};

/// The setting key naming the device that fires time-based cues.
pub const KEY: &str = "notifier";

/// The device the user chose, if they chose one.
///
/// `None` means no choice has been made — not that no device notifies. The
/// caller applies [`default_rule`], and the difference between the two is what
/// §8.9 exists to preserve.
pub fn chosen(state: &Store) -> Result<Option<String>> {
    state.setting(KEY)
}

/// What to do when nobody has chosen.
///
/// One device, so it is this one. The rule is stated as a rule rather than
/// written as a setting, and it stays that way until there is a second device
/// to choose between — which is M3.
pub fn default_rule() -> &'static str {
    "this device — the only one paired"
}

/// Whether a cue type can be observed here.
///
/// The honest answer per cue, on this platform. `false` is not a failure: it
/// means the intent waits for a device that can see it, which is the shape
/// §2.4.1 already describes for a geofence.
pub fn observable(cue_type: &str) -> bool {
    match cue_type {
        // A clock and a branch are both readable from here.
        "time" | "recurring" | "device_wake" | "git_branch" => true,

        // Not observable on this target, and saying so is the point. An intent
        // that claimed to watch this would be registered, reported as working,
        // and permanently silent.
        "app_focus" => false,

        // A phone's to see (§2.4.1), and there is no phone until M4.
        "location" => false,

        // Needs a calendar read that M2 does not have.
        "calendar_start" => false,

        _ => false,
    }
}

/// Say something, by whatever means this desktop offers.
///
/// Best-effort and deliberately not a dependency: a notification that fails is
/// worth a log line, and a missing notification daemon must not stop the
/// process that noticed the cue.
pub fn notify(summary: &str, body: &str) {
    let sent = std::process::Command::new("notify-send")
        .args(["--app-name=brain", summary, body])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    if !sent {
        // The fallback is not silence. An intent that fired and could not be
        // delivered is exactly the thing somebody needs to be able to find out
        // about afterwards.
        tracing::warn!(summary, body, "an intent fired and could not be delivered");
    }
}

/// Where fired intents are remembered.
///
/// Local state rather than the log. *Which* device delivered a notification and
/// *when* it managed to is not a fact about the archive — it is operational
/// chatter, and §2.5.2 keeps that out of a log meant to be worth reading in ten
/// years. It also means two devices can each fire their own local cues without
/// arguing about it.
pub fn already_fired(paths: &StorePaths, id: &[u8]) -> Result<bool> {
    let ops = crate::ops::Ops::open_read(&paths.ops)?;
    ops.intent_fired(id)
}

/// Remember that one fired.
pub fn mark_fired(paths: &StorePaths, id: &[u8], now: u64) -> Result<()> {
    let ops = crate::ops::Ops::open(&paths.ops)?;
    ops.mark_intent_fired(id, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cue_this_device_cannot_see_is_refused() {
        // Declaring a cue type requires being able to observe it. Accepting
        // `app_focus` here would produce an intent that is registered, reported
        // as working, and silently never fires.
        assert!(!observable("app_focus"), "GNOME/Wayland cannot see this");
        assert!(!observable("location"), "no phone until M4");
        assert!(!observable("calendar_start"));
    }

    #[test]
    fn the_cues_a_desktop_can_see_are_accepted() {
        for cue in ["time", "recurring", "device_wake", "git_branch"] {
            assert!(observable(cue), "{cue} is readable from a desktop");
        }
    }

    #[test]
    fn an_unknown_cue_is_not_quietly_accepted() {
        assert!(!observable("telepathy"));
    }
}
