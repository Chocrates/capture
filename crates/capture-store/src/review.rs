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

/// A session, assembled and marked as offered.
///
/// The whole of what a surface needs to draw one. It lives here rather than in
/// each surface because it is more than a query: it picks the items, works out
/// which of them carry a reading worth mentioning, gathers what has lapsed
/// repeatedly, and *writes* that the items were shown — and the order of those
/// matters. The shown-marking goes last, so a failure to read cannot spend a
/// session nobody saw.
///
/// A second copy of that sequence on the phone would be a phone that spent
/// sessions differently from the desktop, on the same archive, with no test
/// able to notice.
#[derive(Debug)]
pub struct Offered {
    pub items: Vec<Suggested>,
    /// Notes that have lapsed repeatedly. Named rather than counted, because
    /// "three things keep slipping" is not actionable and their names are.
    pub nagging: Vec<String>,
}

/// One offered note, with a reading where the archive has one worth showing.
#[derive(Debug)]
pub struct Suggested {
    pub slug: String,
    pub text: Option<String>,
    pub because: Because,
    /// What this might be, where a parse was confident enough to be worth
    /// saying and not confident enough to have acted on its own.
    pub suggestion: Option<String>,
}

/// "This could be Thu 15 Oct or Thu 22 Oct", for a capture that left its day
/// open (grammar 9).
fn day_asked(s: &Session, tgt: &capture_core::EventId) -> Option<String> {
    let [a, b] = s.state.unclear_day_of(tgt).ok().flatten()?;
    let label = |d: &str| capture_time::FloatingDate::parse(d).map(crate::calendar::day_label);
    Some(format!(
        "This could be {} or {}, so it is on neither yet. Which did you mean?",
        label(&a)?,
        label(&b)?
    ))
}

/// Open a review session: choose, read, and record that it was offered.
pub fn open(paths: &StorePaths, now: i64) -> Result<Offered> {
    let mut s = Session::open(paths.clone())?;
    let chosen = session(&s, now)?;

    let items: Vec<Suggested> = chosen
        .iter()
        .map(|i| {
            Suggested {
            slug: i.note.slug.clone(),
            // A day left open comes first: it is why nothing was promoted.
            //
            // Only where nothing was promoted: once a note has a type, a
            // competing reading is noise rather than an offer.
            suggestion: day_asked(&s, &i.note.tgt).or(match (
                s.state.promotion_of(&i.note.tgt).ok().flatten(),
                i.note.text.as_deref(),
            ) {
                (None, Some(text)) => suggestion_for(text, i.note.tz.as_deref(), now),
                // Promoted, but to a day or a rule with no time, because the
                // hour said neither am nor pm. Worth a line here: a review is
                // where somebody is already looking at what they wrote.
                (Some(p), Some(text)) => crate::promote::asks_hour(Some(text), Some(&p)).map(|h| {
                    format!(
                        "\"at {h}\" has no am or pm, so this has no time yet. Which did you mean?"
                    )
                }),
                _ => None,
            }),
            text: i.note.text.clone(),
            because: i.because,
        }
        })
        .collect();

    let nagging = s
        .state
        .repeatedly_lapsed(now, 3)?
        .into_iter()
        .map(|tgt| Ok(s.state.slug_of(&tgt)?.unwrap_or_default()))
        .collect::<Result<Vec<_>>>()?;

    // Written after everything above has succeeded, so a failure to read
    // cannot spend a session nobody got to see — and in one commit, because
    // five of them is a third of a second and no surface can draw until they
    // are done.
    let tgts: Vec<_> = chosen.iter().map(|i| i.note.tgt).collect();
    shown(&mut s, &tgts)?;

    Ok(Offered { items, nagging })
}

/// What the parser thinks this might be, when that is worth saying.
///
/// Nothing at all below any confidence, and nothing at or above the promotion
/// floor — at that point the archive has already acted, and repeating it as a
/// suggestion invites somebody to confirm a decision that was not theirs to
/// make.
///
/// Derived rather than stored: a suggestion is a guess, and writing one into
/// the log would make an event out of it.
pub fn suggestion_for(text: &str, tz: Option<&str>, now: i64) -> Option<String> {
    let zone = tz
        .and_then(capture_time::Zone::new)
        .unwrap_or_else(|| capture_time::Zone::new("UTC").expect("UTC is always present"));

    // A capture that nearly was a list, before anything about time.
    //
    // First because it is the more specific statement: a grocery list missing
    // its colon also reads as "a day" at 70%, and telling somebody their
    // shopping list might be an appointment is worse than useless when the
    // actual answer is a missing character.
    if let Some(miss) = capture_parse::near_miss(text) {
        return Some(miss.said());
    }

    let r = capture_parse::recognize(text, now, zone);
    if r.confidence == 0 || r.confidence >= crate::promote::FLOOR {
        return None;
    }

    let day = crate::calendar::day_label;
    let what = match &r.shape {
        capture_parse::Shape::Task { due: None } => String::from("a task"),
        capture_parse::Shape::Task { due: Some(d) } => format!("a task due {}", day(*d)),
        // "All day" said, because that is what accepting writes: "something on
        // 2026-10-15" read as agreeing with a text that said 4pm (2026-10-13).
        capture_parse::Shape::Point(capture_parse::When::Date(d)) => {
            format!("something all day on {}", day(*d))
        }
        capture_parse::Shape::Point(capture_parse::When::Moment(ms)) => {
            let (h, m) = capture_time::civil_clock(*ms, zone);
            format!(
                "something on {} at {h:02}:{m:02}",
                day(capture_time::civil_date(*ms, zone))
            )
        }
        capture_parse::Shape::Range { from, to } => {
            format!("something from {} to {}", day(*from), day(*to))
        }
        capture_parse::Shape::Recurring { .. } => String::from("something recurring"),
        capture_parse::Shape::Nothing => return None,
    };

    // A time in the text that this reading does not use. Accepting would drop
    // it, and the person should know that before saying yes.
    let timed = matches!(
        r.shape,
        capture_parse::Shape::Point(capture_parse::When::Moment(_))
            | capture_parse::Shape::Recurring {
                at_minute: Some(_),
                ..
            }
    );
    let unread = (!timed)
        .then(|| {
            capture_parse::tokenize(text)
                .into_iter()
                .find(|t| matches!(t.class, capture_parse::Class::Clock { .. }))
                .map(|t| format!(" It did not read \"{}\".", t.text))
        })
        .flatten()
        .unwrap_or_default();

    Some(format!(
        "This might be {what} — {}% sure, which is under the bar for deciding on its own.{unread}",
        r.confidence
    ))
}
