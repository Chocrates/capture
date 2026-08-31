//! Resurfacing notes on a schedule, without keeping score.
//!
//! # The component with the cleanest evidence, and the one most able to harm
//!
//! Scheduled retrieval practice under imposed pacing is the one part of this
//! design with a clean mapping to a finding (§0.1 finding 2). It is also the
//! surface where a graded record would do the most damage: a recurring,
//! indefinitely repeating, permanently recorded history of memory failures,
//! delivered on a schedule the user did not set, to somebody whose stated
//! difficulty is memory.
//!
//! §2.7 resolved that by removing the grade rather than the feature. The
//! outcome is *shown / engaged / deferred* — whether it appeared, whether you
//! did anything, whether you pushed it away. Nothing here knows whether you
//! remembered, because nothing here asks.
//!
//! # Bounded, and it ends
//!
//! Five items, then the session is over and says so. An unbounded queue is a
//! backlog, and §3.6 spends its length on why a manufactured backlog is the
//! wrong shape for this user. A session you always finish cannot become one.
//!
//! # Scheduling by engagement rather than accuracy
//!
//! Ordinary spaced repetition schedules on whether you got an item right. There
//! is no right here. An item you engage with recurs less often because it has
//! landed; an item you defer twice stops being offered rather than being
//! pressed — somebody who has declined it twice has told you.

use capture_core::EventId;
use capture_core::body::{Outcome, ReviewBody};
use capture_core::event::Kind;

use crate::projection::NoteRow;
use crate::session::{Session, encode};
use crate::{Result, StorePaths};

/// How many items a session offers.
///
/// §2.8 asks for "a fixed small number" and declines to choose one. Five.
///
/// The property that matters is that it is always finishable: a session you do
/// not finish is a backlog by another name. Five is small enough to complete on
/// any day and still deep enough to reach past the first selection tier. Ten
/// reaches further and is abandonable halfway; three guarantees completion and
/// means most of the selection logic never runs.
///
/// Deliberately not a setting. A knob nobody turns is a default with extra
/// code, and §0.2 rates the cost of a decision highly enough that offering one
/// here is not free.
pub const SESSION: usize = 5;

/// Why a note was chosen, in the priority order §2.8 states.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Because {
    /// It carries a forward marker the recognizer found — "revisit Q2".
    Forward,
    /// It has never been engaged with since it was captured.
    Never,
    /// Its ambient context matches the current one.
    Context,
}

/// One item in a session.
#[derive(Clone, Debug)]
pub struct Item {
    /// The note.
    pub note: NoteRow,
    /// Why it was offered.
    pub because: Because,
}

/// Choose what to offer.
///
/// Muted notes never appear (§2.6), notes declined twice never appear, and a
/// note is only offered again once its interval has elapsed. Redacted and
/// withheld notes are excluded by having no readable text to resurface.
pub fn session(s: &Session, now: i64) -> Result<Vec<Item>> {
    let mut out: Vec<Item> = Vec::new();

    for (because, rows) in [
        (Because::Forward, s.state.review_forward(now, SESSION)?),
        (Because::Never, s.state.review_never_engaged(now, SESSION)?),
        (Because::Context, s.state.review_by_context(now, SESSION)?),
    ] {
        for note in rows {
            if out.len() >= SESSION {
                return Ok(out);
            }
            if out.iter().any(|i| i.note.tgt == note.tgt) {
                continue;
            }
            out.push(Item { note, because });
        }
    }

    Ok(out)
}

/// How long before a note is offered again.
///
/// Expanding intervals, driven by how many times something has been done with
/// it. Not by accuracy — there is no accuracy here, and §2.7 removed the graded
/// outcome such a schedule would need.
pub fn interval_ms(engaged: u32) -> i64 {
    const DAY: i64 = 24 * 60 * 60 * 1000;
    match engaged {
        0 => DAY,
        1 => 3 * DAY,
        2 => 7 * DAY,
        3 => 21 * DAY,
        _ => 60 * DAY,
    }
}

/// Record what became of an offered note.
pub fn record(paths: &StorePaths, name: &str, outcome: Outcome) -> Result<()> {
    let mut s = Session::open(paths.clone())?;
    let tgt = crate::mutate::find(&s, name)?;
    write(&mut s, tgt, outcome)
}

/// Record that several notes were offered, in one commit.
///
/// A session offers five notes and each is a `shown` event. Written one at a
/// time that is five commits — about a third of a second — and it happens
/// before the pane can draw, so it lands as a delay on opening a tab.
pub fn shown(s: &mut Session, tgts: &[EventId]) -> Result<()> {
    s.append_all(Kind::REVIEW, tgts, |nonce, _| {
        encode(&ReviewBody {
            nonce,
            outcome: Outcome::Shown,
        })
    })?;
    Ok(())
}

/// The same, on a session that is already open.
pub fn write(s: &mut Session, tgt: EventId, outcome: Outcome) -> Result<()> {
    s.append(Kind::REVIEW, Some(tgt), None, |nonce| {
        encode(&ReviewBody { nonce, outcome })
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_is_five_items() {
        // §2.8 asks for a fixed small number and does not choose one. Written
        // down here so changing it is a decision rather than a drift.
        assert_eq!(SESSION, 5);
    }

    #[test]
    fn intervals_expand_with_engagement() {
        // An item you engage with recurs less often because it has landed.
        let mut last = 0;
        for engaged in 0..6 {
            let next = interval_ms(engaged);
            assert!(next >= last, "intervals must not shorten with engagement");
            last = next;
        }
        assert!(interval_ms(5) > interval_ms(0) * 10);
    }

    #[test]
    fn nothing_here_scores_anything() {
        // §2.7, as a check on this module's own surface. The only inputs to
        // scheduling are counts of what somebody did; there is no accuracy, no
        // streak, and no aggregate to display.
        //
        // Stated as a test because the absence is a requirement rather than an
        // omission, and an omission is exactly what a later change would fill.
        // Code only. A substring scan over the whole file cannot tell "there is
        // deliberately no accuracy here" in a comment from an `accuracy` field
        // in a struct, and this file argues the former at length — the first
        // version of this test failed on its own prose, then on the list of
        // words it was looking for.
        let whole = include_str!("review.rs");
        let above_tests = whole.split("#[cfg(test)]").next().unwrap_or(whole);
        let code: String = above_tests
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !t.starts_with("//") && !t.starts_with("///") && !t.starts_with("//!")
            })
            .collect::<Vec<_>>()
            .join("\n");

        for forbidden in ["streak", "accuracy", "grade", "correct"] {
            assert!(
                !code.contains(forbidden),
                "review grew a {forbidden:?} — §2.7 says it must not"
            );
        }
    }

    #[test]
    fn the_priority_order_is_the_one_the_design_states() {
        // Forward markers, then never-engaged, then context. Ord is what the
        // session relies on, so it is pinned rather than assumed.
        assert!(Because::Forward < Because::Never);
        assert!(Because::Never < Because::Context);
    }
}
