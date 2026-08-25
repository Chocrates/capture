//! A ceiling on how much one origin can destroy per hour.
//!
//! # What this is defending against
//!
//! Redaction is the only operation that removes data, and it is designed to
//! propagate: a redaction issued anywhere destroys the named content
//! everywhere, and is retained so that it destroys matching events arriving
//! later. That is exactly what it is for, and it is also what makes it the one
//! event worth a ceiling.
//!
//! Three ways the ceiling earns its place, in rising order of unpleasantness:
//!
//! - a script with a loop in it
//! - a device someone else has picked up
//! - a key that has been taken
//!
//! In all three the archive is being destroyed by something holding a valid
//! key, so nothing downstream refuses it. Each redaction is individually
//! well-formed and individually authorised; the only thing that distinguishes
//! this from ordinary use is the *rate*.
//!
//! # Held, not refused
//!
//! Over the ceiling, redactions are **held for confirmation** rather than
//! rejected. A rejection would put a gap in an origin's sequence, which this
//! design treats as unrecoverable — and it would also be the wrong answer for
//! the innocent case, where somebody meant it and simply had a lot to remove.
//!
//! Holding costs a deliberate act to release and costs nothing to ignore, which
//! is the right shape: a person clearing out a year of notes types one more
//! command, and a runaway loop stops at the ceiling and stays there.
//!
//! # Counted per origin
//!
//! One device going wrong must not spend another's budget. Counting globally
//! would let a compromised key exhaust the ceiling and leave the legitimate
//! device unable to redact anything.

/// How many targets one origin may destroy in an hour before the rest wait.
///
/// Far above deliberate use — clearing out a bad import is tens, not hundreds —
/// and far below what an unattended loop reaches in a second.
pub const TARGETS_PER_HOUR: usize = 256;

/// The window the ceiling is measured over.
pub const WINDOW_MS: u64 = 60 * 60 * 1000;

/// What to do with a redaction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// Within the ceiling. Queue it.
    Proceed,
    /// Over it. Store the event, destroy nothing, and wait to be told.
    Hold {
        /// How many targets this origin has already claimed in the window.
        already: usize,
    },
}

/// One redaction's worth of history: when it was issued, and how many targets.
pub type Issued = (u64, usize);

/// Decide whether a redaction may proceed.
///
/// `history` is the redactions this origin has already issued. Only those
/// inside the window count — an origin that redacted heavily last week has a
/// full allowance today, because the ceiling exists to bound a runaway, not to
/// ration deliberate use.
pub fn check(history: &[Issued], targets: usize, now: u64) -> Verdict {
    let floor = now.saturating_sub(WINDOW_MS);
    let already: usize = history
        .iter()
        .filter(|(at, _)| *at >= floor)
        .map(|(_, n)| n)
        .sum();

    if already + targets > TARGETS_PER_HOUR {
        Verdict::Hold { already }
    } else {
        Verdict::Proceed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    #[test]
    fn an_ordinary_redaction_proceeds() {
        assert_eq!(check(&[], 1, NOW), Verdict::Proceed);
    }

    #[test]
    fn clearing_out_a_lot_at_once_still_proceeds() {
        // Deliberate bulk use is tens of notes, and must not need a second
        // command — a ceiling that ordinary use trips is a ceiling people learn
        // to work around.
        assert_eq!(check(&[], 60, NOW), Verdict::Proceed);
    }

    #[test]
    fn the_ceiling_itself_is_allowed() {
        assert_eq!(check(&[], TARGETS_PER_HOUR, NOW), Verdict::Proceed);
    }

    #[test]
    fn one_past_the_ceiling_is_held() {
        assert!(matches!(
            check(&[], TARGETS_PER_HOUR + 1, NOW),
            Verdict::Hold { .. }
        ));
    }

    #[test]
    fn earlier_redactions_count_towards_the_ceiling() {
        // The runaway case: individually small, collectively a wipe.
        let history: Vec<Issued> = (0..50).map(|i| (NOW - i, 5)).collect();
        assert!(matches!(check(&history, 10, NOW), Verdict::Hold { already } if already == 250));
    }

    #[test]
    fn a_full_allowance_returns_once_the_window_passes() {
        // The ceiling bounds a runaway; it does not ration deliberate use. An
        // origin that cleared out a bad import last week starts today fresh.
        let history = vec![(NOW - WINDOW_MS - 1, TARGETS_PER_HOUR)];
        assert_eq!(check(&history, 10, NOW), Verdict::Proceed);
    }

    #[test]
    fn the_window_edge_still_counts() {
        // Exactly on the boundary is inside it. The other way round leaves a
        // one-millisecond hole that a loop finds immediately.
        let history = vec![(NOW - WINDOW_MS, TARGETS_PER_HOUR)];
        assert!(matches!(check(&history, 1, NOW), Verdict::Hold { .. }));
    }

    #[test]
    fn a_hold_says_how_much_was_already_spent() {
        // The number a person needs in order to decide whether this was them.
        let history = vec![(NOW - 1000, 200)];
        match check(&history, 100, NOW) {
            Verdict::Hold { already } => assert_eq!(already, 200),
            other => panic!("expected a hold, got {other:?}"),
        }
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_open_the_gate() {
        // `now` before the history is a corrected clock. Saturating the floor
        // at zero means every earlier redaction still counts, which errs
        // towards holding — the safe direction for the one irreversible thing
        // in the system.
        let history = vec![(NOW, TARGETS_PER_HOUR)];
        assert!(matches!(check(&history, 1, 1000), Verdict::Hold { .. }));
    }
}
