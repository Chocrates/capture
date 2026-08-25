//! Work that becomes due at a time rather than on a request.
//!
//! # No daemon, no timer unit
//!
//! Three things here happen on a deadline: a redaction becomes due to destroy,
//! a pre-redaction snapshot becomes due to expire, and a silent capture gesture
//! becomes worth mentioning. None of them needs a process of its own.
//!
//! A fourth binary would be a fourth thing that can be silently not running, and
//! the failure it produces is the worst kind: redactions sitting undestroyed
//! while every interface reports them complete. An external timer unit is no
//! better — it puts the schedule somewhere `brain status` cannot see, so the
//! same failure becomes invisible in a different place.
//!
//! So the queue is drained at two moments:
//!
//! - **every process startup**, which is the floor. Any command touching the
//!   store finishes overdue work first.
//! - **a slow tick in the resident overlay**, which is what makes a deadline
//!   land near its time rather than at the next time somebody runs a command.
//!
//! The floor is what makes the guarantee. The tick is what makes it prompt.
//!
//! # Overdue is normal, not exceptional
//!
//! A machine that was switched off for a month comes back with a month of
//! deadlines behind it. Every one of them fires immediately, because the
//! deadline was computed from the redaction's own timestamp: the window has
//! already elapsed, and granting a fresh one would mean the longer a device is
//! away, the longer the content it was told to destroy survives.

use brain_core::EventId;

/// A deadline that has arrived.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Work {
    /// Destroy a note's content.
    Destroy {
        /// The redaction that ordered it.
        redact: EventId,
        /// The note whose content goes.
        target: EventId,
    },
    /// Throw away a pre-redaction snapshot whose retention has elapsed.
    ExpireSnapshot {
        /// The redaction the snapshot was taken for.
        redact: EventId,
    },
}

/// What was found, and what it means for `brain status`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Drained {
    /// Redactions carried out.
    pub destroyed: usize,
    /// Snapshots discarded.
    pub expired: usize,
    /// Interrupted purges finished.
    pub resumed: usize,
    /// Redactions stopped by the hourly ceiling, waiting to be confirmed.
    pub held: usize,
}

impl Drained {
    /// Whether anything at all happened.
    pub fn any(&self) -> bool {
        self.destroyed > 0 || self.expired > 0 || self.resumed > 0 || self.held > 0
    }
}

/// Decide which queued redactions are due.
///
/// `now` is passed in rather than read, so every interesting moment — a second
/// before, a second after, a month late — is reachable without waiting.
pub fn due(queued: &[(EventId, EventId, u64, bool)], now: u64) -> Vec<Work> {
    queued
        .iter()
        .filter(|(_, _, _, cancelled)| !cancelled)
        .filter(|(_, _, execute_after, _)| *execute_after <= now)
        .map(|(redact, target, _, _)| Work::Destroy {
            redact: *redact,
            target: *target,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    fn id(b: u8) -> EventId {
        EventId([b; 32])
    }

    #[test]
    fn a_redaction_inside_its_window_is_not_due() {
        // The cancellation window exists to be usable. Firing early would make
        // it decorative.
        let q = vec![(id(1), id(2), NOW + 1, false)];
        assert!(due(&q, NOW).is_empty());
    }

    #[test]
    fn a_redaction_whose_window_has_elapsed_is_due() {
        let q = vec![(id(1), id(2), NOW, false)];
        assert_eq!(
            due(&q, NOW),
            vec![Work::Destroy {
                redact: id(1),
                target: id(2)
            }]
        );
    }

    #[test]
    fn a_long_overdue_redaction_fires_immediately() {
        // The machine was off for a month. The deadline came from the
        // redaction's own timestamp, so it is simply long past — and granting a
        // fresh window here would mean the longer a device is away, the longer
        // the content it was told to destroy survives.
        let q = vec![(id(1), id(2), NOW - 30 * 24 * 60 * 60 * 1000, false)];
        assert_eq!(due(&q, NOW).len(), 1);
    }

    #[test]
    fn a_cancelled_redaction_is_never_due() {
        let q = vec![(id(1), id(2), NOW - 1, true)];
        assert!(due(&q, NOW).is_empty());
    }

    #[test]
    fn the_deadline_is_inclusive() {
        // Exactly on the boundary counts as arrived. A strict comparison would
        // leave a redaction waiting for the next tick for no reason anybody
        // could observe.
        let q = vec![(id(1), id(2), NOW, false)];
        assert_eq!(due(&q, NOW).len(), 1);
    }

    #[test]
    fn several_due_items_all_come_back() {
        let q = vec![
            (id(1), id(2), NOW - 1, false),
            (id(3), id(4), NOW - 2, false),
            (id(5), id(6), NOW + 100, false),
        ];
        assert_eq!(due(&q, NOW).len(), 2);
    }

    #[test]
    fn nothing_queued_is_not_a_problem() {
        assert!(due(&[], NOW).is_empty());
        assert!(!Drained::default().any());
    }
}
