//! Whether the capture gesture is still reaching anything.
//!
//! # The failure this exists for
//!
//! A registered hotkey can stop working without anything reporting an error.
//! The desktop still lists the binding. The overlay is still installed. Nothing
//! logs a failure, because from every component's point of view nothing failed:
//!
//! - the command the binding runs was moved, renamed, or rebuilt elsewhere
//! - the overlay is not running, so the client connects to nothing
//! - another application claimed the same chord and won
//! - the settings daemon dropped the binding across an upgrade
//!
//! Every one of those looks identical from the settings panel, which will
//! happily show a chord bound to a command that does nothing. The person who
//! set it up finds out weeks later, having assumed for weeks that they were
//! capturing things.
//!
//! A spawn-based binding has more ways to land in this state than a grab does,
//! which is why the mechanism that works on the target machine is also the one
//! that most needs watching.
//!
//! # Confirmed once, then watched
//!
//! Registration ends by asking for the chord to be pressed, because "the
//! setting was written" and "the gesture works" are different claims and only
//! the second one matters. After that, the pair of timestamps below is enough:
//! the gesture is suspect when the device has clearly been in use and the chord
//! has not been seen in a long time.
//!
//! Idleness is not suspicion. A chord that has not fired in a fortnight means
//! nothing on a machine that spent the fortnight switched off.

/// How long a working gesture may stay silent before it is worth asking about.
///
/// Long enough that an ordinary quiet week says nothing, short enough that the
/// answer still arrives while the missing captures could plausibly be
/// remembered. This is a prompt to check, never an error.
pub const SILENCE_MS: u64 = 14 * 24 * 60 * 60 * 1000;

/// What the recorded timestamps say about the gesture.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Health {
    /// No mechanism is registered, so there is nothing to be silent.
    NotRegistered,
    /// Registered, and never once seen to work.
    NeverConfirmed,
    /// Working, or at least recently enough that silence proves nothing.
    Working,
    /// It worked, and has been silent while the device was plainly in use.
    Silent {
        /// How long since it last fired.
        silent_ms: u64,
    },
}

/// Judge the gesture from what has been recorded.
///
/// `now`, `last_fired` and `last_active` are passed in rather than read here so
/// the rule can be exercised at every interesting moment without waiting for
/// one to arrive.
pub fn health(
    registered: bool,
    now: u64,
    last_fired: Option<u64>,
    last_active: Option<u64>,
) -> Health {
    if !registered {
        return Health::NotRegistered;
    }

    let Some(fired) = last_fired else {
        return Health::NeverConfirmed;
    };

    let silent_ms = now.saturating_sub(fired);
    if silent_ms < SILENCE_MS {
        return Health::Working;
    }

    // Silence only means something against use. A device that has been off, or
    // simply unused, has a silent chord for the least interesting reason there
    // is — and reporting that would train the reader to ignore the one case
    // where the silence matters.
    match last_active {
        Some(active) if active > fired => Health::Silent { silent_ms },
        _ => Health::Working,
    }
}

impl Health {
    /// What to tell the user, if anything.
    pub fn message(&self, chord: &str) -> Option<String> {
        match self {
            Health::NotRegistered | Health::Working => None,
            Health::NeverConfirmed => Some(format!(
                "the capture chord ({chord}) has never been seen to work.\n\
                 Press it now — if nothing appears, run `capture-overlay --register` again"
            )),
            Health::Silent { silent_ms } => Some(format!(
                "the capture chord ({chord}) has not reached the application in {} days,\n\
                 although this device has been in use.\n\
                 Something may have taken the chord, or the overlay may not be running.",
                silent_ms / (24 * 60 * 60 * 1000)
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 24 * 60 * 60 * 1000;
    const NOW: u64 = 1_800_000_000_000;

    #[test]
    fn nothing_registered_is_not_a_condition() {
        // A desktop with no way to register a global chord is a supported
        // situation, and the terminal chord is unaffected.
        assert_eq!(health(false, NOW, None, None), Health::NotRegistered);
    }

    #[test]
    fn registered_but_never_seen_to_work_is_reported() {
        // The state a failed registration leaves behind, and the one most
        // likely to be believed working: the setting exists, so everything
        // looks right.
        assert_eq!(health(true, NOW, None, Some(NOW)), Health::NeverConfirmed);
    }

    #[test]
    fn a_recently_used_chord_is_working() {
        let fired = NOW - DAY;
        assert_eq!(health(true, NOW, Some(fired), Some(NOW)), Health::Working);
    }

    #[test]
    fn a_long_silence_on_a_used_device_is_reported() {
        // The failure worth catching: captures have been happening by other
        // means, and the chord has not been among them.
        let fired = NOW - 30 * DAY;
        let active = NOW - DAY;
        match health(true, NOW, Some(fired), Some(active)) {
            Health::Silent { silent_ms } => assert_eq!(silent_ms, 30 * DAY),
            other => panic!("expected a silent gesture, got {other:?}"),
        }
    }

    #[test]
    fn a_long_silence_on_an_unused_device_is_not() {
        // The machine was off. Reporting this is how a health surface teaches
        // the reader to ignore it.
        let fired = NOW - 30 * DAY;
        let active = NOW - 30 * DAY - 1;
        assert_eq!(
            health(true, NOW, Some(fired), Some(active)),
            Health::Working
        );
    }

    #[test]
    fn silence_with_no_recorded_activity_is_not_reported() {
        // Nothing is known about whether the device was used, so nothing can
        // be concluded. Guessing would produce a warning from an absence.
        let fired = NOW - 30 * DAY;
        assert_eq!(health(true, NOW, Some(fired), None), Health::Working);
    }

    #[test]
    fn the_threshold_is_not_crossed_a_moment_early() {
        let fired = NOW - SILENCE_MS + 1;
        assert_eq!(health(true, NOW, Some(fired), Some(NOW)), Health::Working);
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_produce_a_warning() {
        // `now` earlier than the last firing is a corrected clock, not a dead
        // gesture, and a saturating subtraction keeps it from becoming an
        // enormous silence.
        let fired = NOW + 10 * DAY;
        assert_eq!(health(true, NOW, Some(fired), Some(NOW)), Health::Working);
    }

    #[test]
    fn only_the_reportable_states_have_a_message() {
        assert!(Health::NotRegistered.message("Ctrl-Space").is_none());
        assert!(Health::Working.message("Ctrl-Space").is_none());
        assert!(Health::NeverConfirmed.message("Ctrl-Space").is_some());
        assert!(
            Health::Silent {
                silent_ms: 30 * DAY
            }
            .message("Ctrl-Space")
            .is_some()
        );
    }

    #[test]
    fn a_message_names_the_chord_and_what_to_do() {
        // Being told a gesture is broken without being told which gesture, or
        // what to try, is a notification that costs attention and returns none.
        let m = Health::NeverConfirmed.message("Ctrl-Super-Space").unwrap();
        assert!(m.contains("Ctrl-Super-Space"), "{m}");
        assert!(m.contains("capture-overlay --register"), "{m}");
    }
}
