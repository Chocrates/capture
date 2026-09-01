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
//! as a user decision — `capture status` would then report it as "set by you", it
//! could never be centrally improved because changing the default would not
//! reach anybody, and there is no way to tell it apart from a choice somebody
//! actually made.
//!
//! So: until the user chooses, the notifier is decided by a rule evaluated
//! locally and written nowhere. [`chosen`] returns `None` in that case, and it
//! is `None` that `capture status` reports as a default.
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
        // A clock is readable anywhere.
        "time" | "recurring" | "device_wake" => true,

        // Where the repository is, which is not a phone.
        "git_branch" => !ANDROID,

        // Not observable on GNOME/Wayland, and saying so is the point. An
        // intent that claimed to watch this would be registered, reported as
        // working, and permanently silent. Nor on Android, where there is no
        // notion of another application's focus available to this one.
        "app_focus" => false,

        // A phone's to see (§2.4.1). False on the desktop not because the
        // feature is missing but because the desktop cannot observe it: a
        // geofence answered by a machine that does not move is a geofence that
        // never fires, or fires once and never stops.
        //
        // True here says only that the *platform* can observe it. Whether this
        // particular device may is a runtime question — the permission — and
        // one a store cannot answer. The surface that knows reports it (§7.3),
        // because an intent failing invisibly is worse than no intent: the
        // person believes something is watching when nothing is.
        "location" => ANDROID,

        // Needs a calendar read that M2 does not have.
        "calendar_start" => false,

        _ => false,
    }
}

/// Whether this build is the phone.
///
/// The same function, answering differently where it runs. A device declares
/// what it can see, and these two see different things — which is the whole
/// reason §2.4.1 routes a cue to a device rather than to "the notifier".
const ANDROID: bool = cfg!(target_os = "android");

/// Say something, by whatever means this desktop offers.
///
/// Best-effort and deliberately not a dependency: a notification that fails is
/// worth a log line, and a missing notification daemon must not stop the
/// process that noticed the cue.
pub fn notify(summary: &str, body: &str) {
    let sent = std::process::Command::new("notify-send")
        .args(["--app-name=capture", summary, body])
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

/// How long a repeating cue waits before it may say the same thing again.
///
/// Not a debounce on the sensor — the platform already collapses a boundary
/// crossing into one event. This is about the boundary itself: a shop is a
/// point with a radius, a person at the counter is metres from the edge of it,
/// and a phone that re-acquires its position every few minutes will cross that
/// edge repeatedly without anybody moving. Thirty minutes is long enough that
/// leaving and coming back is a real visit rather than a wobble.
pub const REARM_MS: u64 = 30 * 60 * 1000;

/// Whether a cue that has already fired may fire again.
///
/// A time cue happens once: it named a moment, the moment passed, and saying
/// so twice is noise. A geofence happens every visit — "buy screws when you
/// are at the hardware shop" is worthless if it only ever works the first
/// time, and that is the difference §2.4 draws between a reminder and a plan.
pub fn may_fire_again(last_fired: Option<u64>, now: u64, cue_type: &str) -> bool {
    match last_fired {
        None => true,
        Some(_) if !repeats(cue_type) => false,
        Some(then) => now.saturating_sub(then) >= REARM_MS,
    }
}

/// Whether a cue describes a recurring condition rather than a single moment.
fn repeats(cue_type: &str) -> bool {
    matches!(
        cue_type,
        "location" | "recurring" | "app_focus" | "git_branch" | "device_wake"
    )
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
    fn a_moment_fires_once_and_a_place_fires_every_visit() {
        // The difference between a reminder and a plan (§2.4). "Buy screws
        // when you are at the hardware shop" is worthless if it only works the
        // first time somebody goes there.
        assert!(may_fire_again(None, 1_000, "time"), "never fired yet");
        assert!(
            !may_fire_again(Some(1_000), 9_999_999_999, "time"),
            "a moment passed once and saying so again is noise"
        );
        assert!(
            may_fire_again(Some(1_000), 1_000 + REARM_MS, "location"),
            "arriving again is a new arrival"
        );
    }

    #[test]
    fn standing_at_the_edge_of_a_fence_does_not_notify_repeatedly() {
        // A shop is a point with a radius; somebody at the counter is metres
        // from the edge, and a phone re-acquiring its position will cross that
        // edge without anybody moving.
        let arrived = 1_700_000_000_000;
        assert!(!may_fire_again(Some(arrived), arrived + 60_000, "location"));
        assert!(!may_fire_again(
            Some(arrived),
            arrived + REARM_MS - 1,
            "location"
        ));
    }

    #[test]
    fn a_cue_this_device_cannot_see_is_refused() {
        // Declaring a cue type requires being able to observe it. Accepting
        // `app_focus` here would produce an intent that is registered, reported
        // as working, and silently never fires.
        assert!(!observable("app_focus"), "no platform here can see this");

        // A geofence is the phone's to observe. Answered by a machine that
        // does not move, it either never fires or fires once and never stops.
        assert_eq!(
            observable("location"),
            cfg!(target_os = "android"),
            "location is observable exactly where there is something that moves"
        );

        // And the converse: a branch is where the repository is.
        assert_eq!(observable("git_branch"), !cfg!(target_os = "android"));
        assert!(!observable("calendar_start"));
    }

    #[test]
    fn a_clock_is_readable_from_any_device() {
        for cue in ["time", "recurring", "device_wake"] {
            assert!(observable(cue), "{cue} needs nothing but a clock");
        }
    }

    #[test]
    #[cfg(not(target_os = "android"))]
    fn a_desktop_can_see_the_repository_it_holds() {
        assert!(observable("git_branch"));
    }

    #[test]
    fn an_unknown_cue_is_not_quietly_accepted() {
        assert!(!observable("telepathy"));
    }
}
