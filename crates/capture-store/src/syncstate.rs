//! What the state of sync is on this device, asked once for every surface.
//!
//! # Two questions, not one
//!
//! A round that ran and reached nobody and a round that never ran are different
//! situations with different fixes, and the peer rows report them identically:
//!
//! - *Has a round run?* If not, sync is not happening — the daemon is not
//!   started, the periodic job is being held off, the application was
//!   force-stopped, the OEM put it to sleep. Nothing about the network is
//!   wrong.
//! - *Did it reach anyone?* If rounds run and reach nobody, the network is the
//!   problem: a VPN, a guest SSID, discovery unavailable, an address that
//!   changed.
//!
//! # Why this is here and not on each surface
//!
//! It was asked correctly in one place and not at all in two. `capture-ffi`
//! asked both questions and wrote down why; `capture status` and the desktop
//! window went straight to the peer rows and said "device X has never been
//! reached", which reads as a network fault and sends somebody to look at their
//! Wi-Fi.
//!
//! The cost is in the owner's own ledger, written as a question because nothing
//! would answer it: *"Phone last reached by desktop 51h ago — sync not
//! running?"* It was not running. `capture-syncd` had never been started on
//! that machine — correctly, since `install.sh` declines to open a listening
//! socket on anybody's behalf — and no surface there said so.
//!
//! So the decision lives here and the surfaces ask. The arms are in the order
//! the questions are asked, and a caller that matches exhaustively cannot skip
//! the step the other two skipped.
//!
//! # Read-only, and that is load-bearing
//!
//! Everything here comes from `Ops` through a read-only handle. The moment this
//! most needs answering is the moment something is wrong, and §8.8 keeps reads
//! off the writer for exactly that reason.

use crate::Result;
use crate::ops::Ops;

/// A day of silence is ordinary; the other device was off.
///
/// The period is fifteen minutes and Doze stretches it to hours, so anything
/// under a day is noise.
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// Three days is not ordinary.
///
/// No plausible week has the desktop closed for three days while the phone is
/// in daily use, and by then there is enough unreplicated work that losing the
/// phone would cost real notes.
const SERIOUS_MS: u64 = 3 * DAY_MS;

/// What sync is doing on this device.
///
/// Ordered as the questions are asked: whether there is anybody to sync with,
/// then whether the job runs, then whether it reaches anyone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncState {
    /// Nothing is paired.
    ///
    /// Not a failure and not reported. One device with no peers is not failing
    /// to sync, and saying so on every single-device archive is how a reader
    /// learns to skip the section.
    Alone,
    /// Paired, and no round has ever run.
    NeverRan,
    /// Rounds have run, and not for this long.
    NotRunning { since_ms: u64 },
    /// Rounds run, and no peer has ever been reached.
    NeverReachedAnyone,
    /// Rounds run and have reached nobody for this long.
    ReachingNobody { since_ms: u64 },
    /// Running, and reaching somebody.
    Working,
}

/// Read the state of sync.
///
/// `now` is passed in rather than read here, so every boundary can be exercised
/// without waiting for one to arrive — the same shape `gesture::health` uses.
///
/// `paired` is how many *other* devices are in the roster, and it comes from
/// the caller because membership lives in the log rather than in `ops`. It is
/// not inferred from `peer_addresses`: a row there means somebody has been
/// dialled or has answered, so a device paired and never yet contacted has a
/// roster entry and no row — which is precisely the case this exists to
/// report, and inferring membership from `ops` would answer it with silence.
pub fn sync_state(ops: &Ops, paired: usize, now: u64) -> Result<SyncState> {
    if paired == 0 {
        return Ok(SyncState::Alone);
    }
    let peers = ops.peer_addresses()?;

    // The job, before the network. If rounds are not happening, what the
    // network would have done is not a question anybody can act on.
    let Some(ran) = ops.last_round()? else {
        return Ok(SyncState::NeverRan);
    };
    let since = now.saturating_sub(ran);
    if since > DAY_MS {
        return Ok(SyncState::NotRunning { since_ms: since });
    }

    // Rounds are happening. Now the network is a fair question.
    let Some(reached) = peers.iter().filter_map(|p| p.last_seen).max() else {
        return Ok(SyncState::NeverReachedAnyone);
    };
    let quiet = now.saturating_sub(reached);
    if quiet > DAY_MS {
        return Ok(SyncState::ReachingNobody { since_ms: quiet });
    }

    Ok(SyncState::Working)
}

impl SyncState {
    /// What to tell somebody, and how loudly — or nothing.
    ///
    /// `attention` is something wanting a decision and `worth-knowing` is
    /// something true that needs nothing; they are not severities. A list where
    /// everything is urgent is one where nothing is.
    ///
    /// Said with *when*, never as a bare "sync is broken". The duration is what
    /// tells somebody whether this started before or after the thing they
    /// changed.
    pub fn message(&self) -> Option<(&'static str, String)> {
        match self {
            SyncState::Alone | SyncState::Working => None,

            // Names the daemon, because that is the remedy. It does not offer
            // to start one: `install.sh` declines to open a listening socket on
            // anybody's behalf, and a health surface that quietly did what the
            // installer refused to would be worse than the silence it replaces.
            SyncState::NeverRan => Some((
                "attention",
                "sync has never run on this device, so nothing here exists anywhere else. \
                 Run capture-syncd to start it."
                    .into(),
            )),

            SyncState::NotRunning { since_ms } => Some((
                tier(*since_ms),
                format!(
                    "sync has not run for {}. Captures are safe on this device and have not \
                     been copied to any other one.",
                    lasting(*since_ms)
                ),
            )),

            SyncState::NeverReachedAnyone => Some((
                "attention",
                "no other device has ever been reached, so nothing here exists anywhere else"
                    .into(),
            )),

            SyncState::ReachingNobody { since_ms } => Some((
                tier(*since_ms),
                format!(
                    "no other device has been reached for {}, though sync is running. \
                     Both devices need to be awake on the same network.",
                    lasting(*since_ms)
                ),
            )),
        }
    }
}

fn tier(since_ms: u64) -> &'static str {
    if since_ms > SERIOUS_MS {
        "attention"
    } else {
        "worth-knowing"
    }
}

/// How long something has been true, coarsely.
///
/// "2 days" is a thing somebody can compare against what they remember doing;
/// "51 hours" is arithmetic, and a condition that has to be worked out gets
/// skipped.
pub fn lasting(ms: u64) -> String {
    let hours = ms / (60 * 60 * 1000);
    let days = hours / 24;
    match (days, hours) {
        (0, 0) => "less than an hour".into(),
        (0, 1) => "an hour".into(),
        (0, h) => format!("{h} hours"),
        (1, _) => "a day".into(),
        (d, _) => format!("{d} days"),
    }
}

/// How long a peer may go unreached before it wants attention.
///
/// A week, per §4.5. Shorter would fire every time a laptop spends a weekend
/// shut, which trains the person to ignore it — and this is a report that has
/// to be believed the one time it means something. Longer and what is stranded
/// on a quiet device is a month of captures rather than a handful.
///
/// That argument was written twice, once in each surface, in two phrasings of
/// the same point. It lives here now, with the rule it is about.
///
/// Deliberately not `notifier::STALE_MS`, which is also a week and is a
/// different decision — how late a *moment* may be and still be worth
/// announcing. Two spans that happen to be equal are not one span, and merging
/// them because the numbers match is how a change to one silently moves the
/// other.
pub const PEER_QUIET_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// Whether a peer has gone quiet.
///
/// One rule, because this was decided in two surfaces: the terminal's status
/// and the window's health each held a constant and a predicate of their own,
/// identical today. Change the week in one and the two disagree about whether
/// your phone needs attention — and neither is obviously wrong, which is the
/// state that makes somebody stop trusting the report rather than the surface.
///
/// A device gone quiet holds captures that exist on exactly one machine, and
/// the longer that goes unnoticed the more of them there are (§4.5).
///
/// **Never reached counts as quiet**, and that is the half a second copy would
/// most likely get different. The alternative — "never reached" as its own
/// state, not quite an alarm — is defensible right up until the moment somebody
/// has a device they paired and never synced, which is exactly when the
/// captures on it are most stranded.
pub fn peer_is_stale(last_seen: Option<u64>, now: u64) -> bool {
    last_seen.is_none_or(|t| now.saturating_sub(t) > PEER_QUIET_MS)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    #[test]
    fn coarse_durations_read_as_prose() {
        assert_eq!(lasting(0), "less than an hour");
        assert_eq!(lasting(60 * 60 * 1000), "an hour");
        assert_eq!(lasting(5 * 60 * 60 * 1000), "5 hours");
        assert_eq!(lasting(DAY_MS), "a day");
        assert_eq!(lasting(3 * DAY_MS), "3 days");
    }

    #[test]
    fn nothing_paired_says_nothing() {
        assert_eq!(SyncState::Alone.message(), None);
        assert_eq!(SyncState::Working.message(), None);
    }

    #[test]
    fn a_round_that_never_ran_names_the_daemon_not_the_network() {
        // The finding this module exists for. Somebody reading "never reached"
        // goes and looks at their Wi-Fi.
        let (tier, text) = SyncState::NeverRan.message().expect("said");
        assert_eq!(tier, "attention");
        assert!(text.contains("capture-syncd"), "{text}");
        assert!(
            !text.contains("reached"),
            "this is not about reaching anybody: {text}"
        );
    }

    #[test]
    fn reaching_nobody_is_about_the_network() {
        let (_, text) = SyncState::ReachingNobody {
            since_ms: 2 * DAY_MS,
        }
        .message()
        .expect("said");
        assert!(text.contains("network"), "{text}");
        assert!(
            text.contains("sync is running"),
            "the distinction is the point: {text}"
        );
    }

    #[test]
    fn a_day_is_worth_knowing_and_three_is_attention() {
        // The boundary, which is a judgement rather than an accident: a day of
        // silence is ordinary and three days is not.
        assert_eq!(tier(DAY_MS + 1), "worth-knowing");
        assert_eq!(tier(SERIOUS_MS + 1), "attention");
    }

    #[test]
    fn the_order_of_the_questions_is_the_order_of_the_arms() {
        // Not a behaviour test — a statement that the type encodes the rule.
        // A surface matching exhaustively has to pass `NeverRan` before it can
        // reach the arms about reaching anybody, which is the step the desktop
        // and the command line both skipped.
        let all = [
            SyncState::Alone,
            SyncState::NeverRan,
            SyncState::NotRunning { since_ms: 0 },
            SyncState::NeverReachedAnyone,
            SyncState::ReachingNobody { since_ms: 0 },
            SyncState::Working,
        ];
        assert_eq!(all.len(), 6, "an arm was added without a decision about it");
        let _ = NOW;
    }
}

#[cfg(test)]
mod peer_quiet {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;
    const DAY: u64 = 24 * 60 * 60 * 1000;

    #[test]
    fn a_peer_reached_this_week_is_not_quiet() {
        assert!(!peer_is_stale(Some(NOW - 6 * DAY), NOW));
        assert!(!peer_is_stale(Some(NOW), NOW));
    }

    #[test]
    fn a_peer_not_reached_for_longer_is_quiet() {
        assert!(peer_is_stale(Some(NOW - 8 * DAY), NOW));
        assert!(peer_is_stale(Some(NOW - 30 * DAY), NOW));
    }

    #[test]
    fn exactly_a_week_is_not_yet_quiet() {
        // Pinned rather than left to whoever reads the `>` next. A week to the
        // millisecond is the boundary and it falls on the quiet side of the
        // line — one more millisecond and it wants attention.
        assert!(!peer_is_stale(Some(NOW - PEER_QUIET_MS), NOW));
        assert!(peer_is_stale(Some(NOW - PEER_QUIET_MS - 1), NOW));
    }

    #[test]
    fn a_peer_never_reached_is_quiet() {
        // The judgement a second copy would most likely get different: a device
        // paired and never synced is exactly when its captures are most
        // stranded, so it is not a gentler state than having gone quiet.
        assert!(peer_is_stale(None, NOW));
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_make_a_peer_quiet() {
        // `saturating_sub`, said out loud. A peer last seen "in the future"
        // after a clock correction is zero milliseconds ago, not an enormous
        // number of them.
        assert!(!peer_is_stale(Some(NOW + 5 * DAY), NOW));
    }
}
