//! What state the store is in when a process opens it.
//!
//! # The check is directional, and that is the whole difficulty
//!
//! Two things record how far this device has written: the log's `heads`, and
//! the identity file's watermark. They are written in that order, deliberately
//! — the event commits first, the watermark after — so that a crash between
//! them leaves the watermark *behind* the log and never ahead of it.
//!
//! That ordering makes the two disagreements mean opposite things:
//!
//! | disagreement | what happened | what to do |
//! |---|---|---|
//! | watermark **behind** the log | a crash between committing and recording | update it and carry on |
//! | watermark **ahead** of the log | the store went backwards | mint a new origin |
//!
//! An earlier version of the design said either disagreement should mint a new
//! incarnation. That is symmetric, and symmetric is wrong: the first row is an
//! ordinary post-crash lag, and treating it as a restore would mint a fresh
//! origin after **every** crash — abandoning the old origin's sequence, filling
//! the roster with dead origins, and doing it at exactly the moment the user is
//! least inclined to trust the tool.
//!
//! # Why "ahead" has to mint a new origin
//!
//! The watermark says this device wrote up to sequence 900. The log has 400. So
//! 401 through 900 exist somewhere — on a peer, in an export — and this device
//! is about to hand out 401 again, to different events.
//!
//! Two different events at one position is the failure the whole
//! single-writer-per-origin design exists to prevent, and it is silent: each
//! device holds one of the pair, both look internally consistent, and the loss
//! is in both directions. A new origin starts at 1 in a space nobody else has
//! touched.
//!
//! This is reachable without any restore at all — deleting `events.db` to
//! "start clean" produces it, and the identity file agrees with itself
//! throughout, so nothing else notices.
//!
//! # Unresolved blocks appends, never capture
//!
//! While the state is unresolved this device must not append: it does not know
//! which positions are safe. Captures still succeed, into the spool, which
//! exists for exactly this shape of problem — input accepted now, sequenced
//! later. Refusing to capture would contradict the one property the system is
//! built around, in order to protect a sequence number.

/// What was found on opening.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// Nothing here yet.
    Fresh,
    /// The log and the identity agree.
    Normal,
    /// The watermark lags the log, which is what a crash looks like.
    ///
    /// Not a restore, and not a question: the log is authoritative about what
    /// was written, so the watermark is simply brought up to date.
    WatermarkBehind {
        /// Where the log actually is.
        log_head: u64,
    },
    /// The store has less than this device remembers writing.
    ///
    /// The dangerous one. Nothing may be appended under the old origin.
    StoreWentBackwards {
        /// What the identity file remembers.
        remembered: u64,
        /// What the log holds.
        log_head: u64,
    },
    /// Events exist, and the key that authored them does not.
    ///
    /// Unresolvable here: membership is keyed by a key this device no longer
    /// has, so it can read the archive and cannot author into it. Pairing is
    /// what fixes this, and pairing does not exist yet.
    IdentityMissing,
}

impl State {
    /// Whether this device may append under its current origin.
    pub fn may_append(&self) -> bool {
        matches!(
            self,
            State::Fresh | State::Normal | State::WatermarkBehind { .. }
        )
    }

    /// Whether a new origin has to be minted before anything is written.
    pub fn needs_new_origin(&self) -> bool {
        matches!(self, State::StoreWentBackwards { .. })
    }

    /// What to tell the user, if anything needs saying.
    pub fn message(&self) -> Option<String> {
        match self {
            State::Fresh | State::Normal => None,
            // Not worth mentioning: it is what a crash looks like, it is
            // handled, and reporting it would teach the reader that ordinary
            // events are alarming.
            State::WatermarkBehind { .. } => None,
            State::StoreWentBackwards {
                remembered,
                log_head,
            } => Some(format!(
                "this store holds less than this device remembers writing \
                 ({log_head} against {remembered}).\n\
                 A new origin will be started so that nothing is written twice to \
                 the same position.\n\
                 Captures are unaffected."
            )),
            State::IdentityMissing => Some(
                "this store has events but no device key, so nothing can be authored here.\n\
                 Captures are being kept in the spool."
                    .into(),
            ),
        }
    }
}

/// Work out the state from what the two records say.
///
/// Both numbers are supplied rather than read, so every combination is
/// reachable in a test — including the ones that need a crash at a precise
/// moment to produce for real.
pub fn detect(has_identity: bool, has_events: bool, remembered: u64, log_head: u64) -> State {
    match (has_identity, has_events) {
        (false, false) => State::Fresh,
        (false, true) => State::IdentityMissing,
        // An identity with no log at all. If it remembers writing anything, the
        // log it wrote to is gone and its positions are still out there.
        (true, false) if remembered > 0 => State::StoreWentBackwards {
            remembered,
            log_head: 0,
        },
        (true, false) => State::Fresh,
        (true, true) => {
            if remembered > log_head {
                State::StoreWentBackwards {
                    remembered,
                    log_head,
                }
            } else if remembered < log_head {
                State::WatermarkBehind { log_head }
            } else {
                State::Normal
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_directory_is_a_fresh_install() {
        assert_eq!(detect(false, false, 0, 0), State::Fresh);
    }

    #[test]
    fn a_key_that_has_written_nothing_is_also_fresh() {
        // A crash between creating the keypair and writing the first event.
        // Nothing was written, so nothing can be at risk.
        assert_eq!(detect(true, false, 0, 0), State::Fresh);
    }

    #[test]
    fn agreement_is_normal() {
        assert_eq!(detect(true, true, 41, 41), State::Normal);
    }

    #[test]
    fn a_lagging_watermark_is_an_ordinary_crash() {
        // The event commits, then the watermark is written. A crash between
        // them lands here, and it is the common case — treating it as a restore
        // would mint a new origin after every crash.
        assert_eq!(
            detect(true, true, 40, 41),
            State::WatermarkBehind { log_head: 41 }
        );
        assert!(detect(true, true, 40, 41).may_append());
        assert!(!detect(true, true, 40, 41).needs_new_origin());
    }

    #[test]
    fn a_lagging_watermark_is_not_worth_reporting() {
        // Reporting an ordinary crash recovery teaches the reader that routine
        // events are alarming, which is how a health surface stops being read.
        assert_eq!(detect(true, true, 40, 41).message(), None);
    }

    #[test]
    fn a_store_holding_less_than_was_written_needs_a_new_origin() {
        // The dangerous direction. Positions 401 to 900 exist somewhere, and
        // this device is about to hand out 401 again to different events.
        let s = detect(true, true, 900, 400);
        assert_eq!(
            s,
            State::StoreWentBackwards {
                remembered: 900,
                log_head: 400
            }
        );
        assert!(
            !s.may_append(),
            "nothing may be written under the old origin"
        );
        assert!(s.needs_new_origin());
    }

    #[test]
    fn deleting_the_log_to_start_clean_is_caught() {
        // Reachable with no restore involved, and the identity file agrees with
        // itself throughout, so nothing else notices.
        let s = detect(true, false, 900, 0);
        assert!(s.needs_new_origin());
    }

    #[test]
    fn events_without_a_key_cannot_be_authored_into() {
        // Readable, not writable. Membership is keyed by a key this device no
        // longer holds, and pairing is what would fix it.
        let s = detect(false, true, 0, 400);
        assert_eq!(s, State::IdentityMissing);
        assert!(!s.may_append());
        // And minting an origin would not help: a new origin under no key is
        // not on the roster either.
        assert!(!s.needs_new_origin());
    }

    #[test]
    fn every_unresolved_state_explains_itself() {
        for s in [
            State::StoreWentBackwards {
                remembered: 900,
                log_head: 400,
            },
            State::IdentityMissing,
        ] {
            let m = s.message().expect("an unresolved state says why");
            assert!(
                m.to_lowercase().contains("capture"),
                "and says captures are safe: {m}"
            );
        }
    }

    #[test]
    fn the_states_that_permit_appending_are_exactly_the_resolved_ones() {
        assert!(State::Fresh.may_append());
        assert!(State::Normal.may_append());
        assert!(State::WatermarkBehind { log_head: 1 }.may_append());
        assert!(!State::IdentityMissing.may_append());
        assert!(
            !State::StoreWentBackwards {
                remembered: 2,
                log_head: 1
            }
            .may_append()
        );
    }
}
