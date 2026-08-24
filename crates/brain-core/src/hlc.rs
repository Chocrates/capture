//! Hybrid logical clock.
//!
//! **Frozen.** The layout and the comparison are hashed into every event id, so
//! neither can change without forking the archive.
//!
//! # What this is for
//!
//! The HLC provides a *total order* over all events that is identical on every
//! replica, while staying human-meaningful — a timestamp still means roughly
//! what it says. Every last-writer-wins rule in the conflict table resolves
//! against it, and the fold sorts by it.
//!
//! # The rules, and two rejected drafts
//!
//! Causality is preserved by one thing and one thing only: on receipt, the local
//! clock advances to at least the incoming value. **Adopt, never clamp.**
//!
//! An earlier design draft clamped incoming timestamps, capping how far a peer
//! could advance the local clock. It looks like prudence and it destroys the
//! guarantee:
//!
//! ```text
//! desktop wall 12:10 → emits E1 hlc=(12:10:00)   // legal: matches its own clock
//! phone   wall 12:00 → ingests E1, clamps, does not adopt
//! phone   user reads E1, edits it → E2 (prev=E1) hlc=(12:01:00)
//! ⇒ hlc(E2) < hlc(E1), although E2 causally follows E1
//! ```
//!
//! Every consequence is silent: the phone's later correction loses every LWW
//! race for the duration of the skew, and a fold sorted by HLC reaches an edit
//! before the version it supersedes.
//!
//! A different draft excluded future-dated events from the *projection*, which
//! made the fold a function of the local wall clock — so three devices showed
//! three different states from one converged log.
//!
//! What remains: adopt on receipt, persist the last emitted value across
//! restart, and **never refuse to issue a stamp**.
//!
//! # Why a wrong clock cannot block authoring
//!
//! An earlier draft refused to author while the logical clock ran more than a
//! tolerance ahead of the wall clock, on the grounds that such events would win
//! every last-writer-wins comparison until the wall clock caught up. That is a
//! true cost, and refusing is still the wrong trade.
//!
//! Adoption is mandatory, so a peer whose clock is wrong — or merely a laptop
//! that has not seen an NTP server this week — really does drag this device's
//! logical clock forward. Refusing to author in that state means the capture
//! gesture fails, on a device that is working perfectly, because of a clock on
//! a different machine. This tool exists to catch things before they are lost;
//! a capture path that can be closed by someone else's clock is not one.
//!
//! So the lead is measured and surfaced — [`HlcState::lead_ms`] feeds a health
//! item — and the event is written regardless. Ordering can be repaired later
//! from a log that has the note in it. Nothing repairs a note that was never
//! captured.

use core::cmp::Ordering;

use minicbor::{Decode, Encode};

use crate::id::Dev;

/// How far the logical clock may run ahead of the wall clock before it is
/// worth telling someone.
///
/// A reporting threshold, never a refusal. Exceeding it means some device's
/// clock is wrong and ordering will be distorted until it is fixed — which is
/// worth saying out loud, and never worth dropping a capture over.
pub const LEAD_REPORT_MS: u64 = 60_000;

/// A hybrid logical clock stamp.
///
/// Ordering is `wall`, then `counter`, then `origin` bytes. The origin tiebreak
/// is what makes the order *total* rather than merely partial — without it two
/// devices authoring in the same millisecond would compare equal, and a
/// last-writer-wins rule would have no winner.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Encode, Decode)]
pub struct Hlc {
    /// Physical milliseconds since the Unix epoch.
    #[n(0)]
    pub wall: u64,
    /// Disambiguates events sharing a millisecond, and carries causality when
    /// the physical clock has not advanced.
    #[n(1)]
    pub counter: u32,
    /// Final tiebreak. Present so the order is total.
    #[n(2)]
    pub origin: Dev,
}

impl PartialOrd for Hlc {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Hlc {
    fn cmp(&self, other: &Self) -> Ordering {
        self.wall
            .cmp(&other.wall)
            .then(self.counter.cmp(&other.counter))
            .then(self.origin.0.cmp(&other.origin.0))
    }
}

/// The mutable clock state a device carries.
///
/// This is a *value*, not a service: `brain-core` cannot read a clock, so the
/// host passes physical time in on every call. That keeps the fold pure while
/// leaving the interesting logic — monotonicity, adoption, the emit ceiling —
/// here where it can be tested without a runtime.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Encode, Decode)]
pub struct HlcState {
    #[n(0)]
    wall: u64,
    #[n(1)]
    counter: u32,
}

impl HlcState {
    /// Initial state for a device that has never authored an event.
    pub const fn new() -> Self {
        HlcState {
            wall: 0,
            counter: 0,
        }
    }

    /// Reconstruct persisted state after a restart.
    ///
    /// Persistence is not optional. Without it a clock correction or a reboot
    /// produces events that duplicate or precede timestamps already written —
    /// on a single device, in the first week.
    pub const fn restore(wall: u64, counter: u32) -> Self {
        HlcState { wall, counter }
    }

    /// The values to persist.
    pub const fn persist(&self) -> (u64, u32) {
        (self.wall, self.counter)
    }

    /// How far the logical clock leads the wall clock, in milliseconds.
    ///
    /// Zero in normal operation. A non-zero value means this device has adopted
    /// a stamp from a machine whose clock is ahead of its own, and events it
    /// authors now will carry that lead. Above [`LEAD_REPORT_MS`] it is worth
    /// reporting; it is never worth refusing to write over.
    pub const fn lead_ms(&self, physical_now: u64) -> u64 {
        self.wall.saturating_sub(physical_now)
    }

    /// Issue a stamp for a new local event.
    ///
    /// `physical_now` is supplied by the host. The result never goes backwards,
    /// even when `physical_now` does.
    ///
    /// **This cannot fail.** Capture is the one thing that must always work, and
    /// a clock — this machine's or a peer's — is never a good enough reason to
    /// refuse to write something down. Skew is reported through [`Self::lead_ms`]
    /// instead.
    pub fn issue(&mut self, physical_now: u64, origin: Dev) -> Hlc {
        if physical_now > self.wall {
            self.wall = physical_now;
            self.counter = 0;
        } else {
            // Physical time did not advance — or went backwards. Monotonicity
            // comes from the counter, which is why a reboot cannot produce a
            // descending stamp.
            self.counter = self.counter.saturating_add(1);
        }

        Hlc {
            wall: self.wall,
            counter: self.counter,
            origin,
        }
    }

    /// Absorb an incoming stamp.
    ///
    /// **Adopts** the incoming value. This is the operation that preserves
    /// causality, and clamping it — however prudent it looks — breaks the only
    /// guarantee an HLC provides.
    pub fn observe(&mut self, incoming: &Hlc, physical_now: u64) {
        let high = physical_now.max(self.wall).max(incoming.wall);

        if high == self.wall && high == incoming.wall {
            self.counter = self.counter.max(incoming.counter).saturating_add(1);
        } else if high == self.wall {
            self.counter = self.counter.saturating_add(1);
        } else if high == incoming.wall {
            self.counter = incoming.counter.saturating_add(1);
        } else {
            self.counter = 0;
        }
        self.wall = high;
    }
}

impl Default for HlcState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(b: u8) -> Dev {
        Dev([b; 32])
    }

    #[test]
    fn order_is_total() {
        let a = Hlc {
            wall: 5,
            counter: 0,
            origin: dev(1),
        };
        let b = Hlc {
            wall: 5,
            counter: 0,
            origin: dev(2),
        };
        // Same instant, same counter, different origin: still ordered.
        assert_ne!(a.cmp(&b), Ordering::Equal);
    }

    #[test]
    fn counter_breaks_a_shared_millisecond() {
        let mut s = HlcState::new();
        let d = dev(1);
        let a = s.issue(1000, d);
        let b = s.issue(1000, d);
        assert!(a < b, "two events in one millisecond must still be ordered");
    }

    #[test]
    fn stamps_never_go_backwards_when_the_clock_does() {
        let mut s = HlcState::new();
        let d = dev(1);
        let a = s.issue(10_000, d);
        // NTP steps the clock backwards.
        let b = s.issue(9_000, d);
        assert!(b > a, "monotonicity must survive a backwards clock");
    }

    #[test]
    fn stamps_survive_a_restart() {
        let mut s = HlcState::new();
        let d = dev(1);
        let a = s.issue(10_000, d);
        let (w, c) = s.persist();

        // Process restarts; wall clock has since moved backwards.
        let mut s2 = HlcState::restore(w, c);
        let b = s2.issue(9_500, d);
        assert!(
            b > a,
            "without persistence a reboot re-issues or precedes existing stamps"
        );
    }

    #[test]
    fn observing_adopts_rather_than_clamps() {
        let mut phone = HlcState::new();
        let p = dev(1);
        let d = dev(2);

        // Phone's clock is ten minutes behind; desktop authors at its own time.
        let e1 = Hlc {
            wall: 12_100_000,
            counter: 0,
            origin: d,
        };
        phone.observe(&e1, 12_000_000);

        // Phone now edits what it just read.
        let e2 = phone.issue(12_000_000, p);

        assert!(
            e2 > e1,
            "an edit must order after the version it supersedes; clamping breaks this"
        );
    }

    #[test]
    fn an_idle_device_can_still_author() {
        // Time passing is not clock skew. An earlier version of `issue`
        // compared the wall clock against the last emission, so a device that
        // had not written for a couple of minutes refused to write ever again —
        // which is to say, every device, after every lunch break. It was found
        // by running the command line rather than by any test here, because
        // every test in this file issues stamps milliseconds apart.
        let mut s = HlcState::new();
        let d = dev(1);
        s.issue(1_000, d);

        let a_year_later = 1_000 + 365 * 24 * 60 * 60 * 1_000;
        let stamp = s.issue(a_year_later, d);
        assert_eq!(stamp.wall, a_year_later);
    }

    #[test]
    fn a_clock_dragged_into_the_future_still_authors() {
        // The trade this design makes. Adoption is mandatory, so a peer with a
        // wrong clock really does drag this one forward — and the answer is to
        // write the event anyway. A capture path that a different machine's
        // clock can close is not a capture path.
        let mut s = HlcState::new();
        let now = 1_000_000;
        let lead = LEAD_REPORT_MS * 10;

        s.observe(
            &Hlc {
                wall: now + lead,
                counter: 0,
                origin: dev(2),
            },
            now,
        );

        let stamp = s.issue(now, dev(1));
        assert!(stamp.wall >= now + lead, "the adopted time is carried");
    }

    #[test]
    fn a_lead_is_measurable_so_it_can_be_reported() {
        // What replaces the refusal. The condition is real and worth telling
        // someone about; it is just never worth losing a note over.
        let mut s = HlcState::new();
        let now = 1_000_000;
        assert_eq!(s.lead_ms(now), 0, "a healthy device leads by nothing");

        s.observe(
            &Hlc {
                wall: now + LEAD_REPORT_MS * 3,
                counter: 0,
                origin: dev(2),
            },
            now,
        );
        assert!(
            s.lead_ms(now) > LEAD_REPORT_MS,
            "and this one is reportable"
        );
    }

    #[test]
    fn a_lead_clears_itself_once_the_wall_clock_catches_up() {
        // Nothing needs resetting. The condition is transient by construction.
        let mut s = HlcState::new();
        let now = 1_000_000;
        let ahead = now + LEAD_REPORT_MS * 10;

        s.observe(
            &Hlc {
                wall: ahead,
                counter: 0,
                origin: dev(2),
            },
            now,
        );
        assert!(s.lead_ms(now) > 0);
        s.issue(ahead, dev(1));
        assert_eq!(s.lead_ms(ahead), 0);
    }
}
