//! Turning a recognition into a `promote`, and keeping it current.
//!
//! # Nothing here runs on the capture path
//!
//! §3.1 is explicit: parsing happens after the commit, the overlay closes on
//! Enter, and the capture latency budget is untouched because nothing waits for
//! this. So this is a sweep over what is already stored, run when a session is
//! open — never something a capture calls into.
//!
//! # The floor lives here, once
//!
//! The recognizer scores and stops. Whether a score is enough to promote is
//! decided in [`promote_from`] and nowhere else, so there is one place to read
//! when the answer is surprising and one place to change if the bands move.
//!
//! # Why a sweep rather than a hook
//!
//! Nothing in the system observes a build, and a parser improving is a build
//! rather than an event. Without a version written into the data, a note
//! captured in March could never benefit from a grammar fix shipped in June —
//! the improvement would only ever reach whatever was captured after it. So
//! every promotion records the grammar it came from, and [`sweep`] reparses
//! anything older.

use brain_core::body::{
    EventFields, PromoteBody, Promoted, RuleSpec, SeriesFields, TaskFields, When as BodyWhen,
};
use brain_core::event::Kind;
use brain_core::fold::Note;
use brain_parse::{CONFIDENCE_FLOOR, Recognition, Shape, When};
use brain_time::Zone;

use crate::session::{Session, encode};
use crate::{Result, StorePaths};

/// The score a recognition needs before anything is written, in hundredths.
///
/// Below this a recognition is a suggestion: shown as one line in review,
/// applied to nothing. That places bare weekdays, vague times of day and every
/// task-verb guess under it deliberately — those are the readings most likely
/// to be wrong, and a wrong suggestion costs a glance where a wrong promotion
/// costs a calendar entry (§3.1.3).
pub const FLOOR: u32 = (CONFIDENCE_FLOOR * 100.0) as u32;

/// Whether a recognition is strong enough to write.
///
/// The single place this is decided.
pub fn clears_floor(r: &Recognition) -> bool {
    r.confidence >= FLOOR
}

/// What a recognition would be written as, if it clears the floor.
///
/// Returns `None` for anything below it, and for the shapes that carry no
/// structure worth recording. A task verb never reaches here: it scores 0.40
/// and is a hint for review, not a type.
pub fn promoted_from(r: &Recognition, zone: Zone, now: i64) -> Option<Promoted> {
    if !clears_floor(r) {
        return None;
    }

    match &r.shape {
        Shape::Nothing | Shape::Task => None,

        Shape::Point(When::Date(d)) => Some(Promoted::Event(EventFields {
            when: BodyWhen::AllDay {
                start_date: format!("{d}"),
                end_date: format!("{d}"),
            },
            location: None,
            attendees: Vec::new(),
        })),

        Shape::Point(When::Moment(ms)) => Some(Promoted::Event(EventFields {
            when: BodyWhen::Timed {
                start_ms: *ms,
                // A duration only where one was written. Inventing the "30m
                // default" §3.3 mentions would put a length on the calendar
                // that nobody chose, and a wrong end time is harder to notice
                // than a missing one.
                end_ms: r.markers.duration_min.map(|m| ms + (m as i64) * 60_000),
                tz: String::from(zone.name()),
            },
            location: None,
            attendees: Vec::new(),
        })),

        Shape::Range { from, to } => Some(Promoted::Event(EventFields {
            when: BodyWhen::AllDay {
                start_date: format!("{from}"),
                end_date: format!("{to}"),
            },
            location: None,
            attendees: Vec::new(),
        })),

        Shape::Recurring { rule, at_minute } => Some(Promoted::Series(SeriesFields {
            when: series_start(*at_minute, zone, now)?,
            rule: rule.clone(),
            // Never inferred. A series that genuinely accumulates — a missed
            // dose, an unpaid invoice — is indistinguishable by phrasing from
            // one that does not, and §3.6 is explicit that guessing "this one
            // really matters" is the guess that must not be made silently. A
            // repeatedly-lapsing series prompts once, in review, instead.
            carries_forward: false,
            split_from: None,
        })),
    }
}

/// The first occurrence of a recognised series.
///
/// A real instant, on the day the capture was made. An earlier version stored
/// minutes past local midnight in `start_ms`, which is not a start: expansion
/// needs a day to count from, and a rule with no anchor cannot be expanded at
/// all.
///
/// A recurrence with no time of day stays a series of days rather than moments
/// — "walk the dog daily" has no 00:00 about it, and storing one would put a
/// midnight on the calendar that nobody wrote.
fn series_start(at_minute: Option<u32>, zone: Zone, now: i64) -> Option<BodyWhen> {
    let today = brain_time::civil_date(now, zone);
    match at_minute {
        Some(minutes) => Some(BodyWhen::Timed {
            start_ms: brain_time::at_local(today, minutes / 60, minutes % 60, zone)?,
            end_ms: None,
            tz: String::from(zone.name()),
        }),
        None => Some(BodyWhen::AllDay {
            start_date: format!("{today}"),
            end_date: format!("{today}"),
        }),
    }
}

/// Whether a note needs reading again.
///
/// Either it has never been promoted, or it was promoted by an older grammar.
/// A pinned promotion is never reparsed: it is somebody's correction, and the
/// whole point of the pin is that no later parse displaces it (§6).
pub fn needs_parse(note: &Note, grammar: u32) -> bool {
    match &note.promotion {
        None => true,
        Some(p) => !p.pinned && p.grammar.is_some_and(|g| g < grammar),
    }
}

/// Read everything that needs reading, and write what clears the floor.
///
/// Returns how many promotions were written. Runs on a session that is already
/// open, so it costs nothing when there is nothing to do.
pub fn sweep(s: &mut Session, now: i64) -> Result<usize> {
    let grammar = brain_parse::GRAMMAR_VERSION;
    let mut written = 0;

    // An indexed query rather than a fold. This folded the whole log until the
    // projection learned to store a promotion; it was honest and temporary and
    // is now neither.
    //
    // Bounded, because a sweep is not the only thing this process has to do. A
    // grammar change over a large store is spread across several startups
    // rather than making one of them slow — nothing is lost by taking two runs
    // to finish, and the alternative is a capture that waits behind a reparse.
    const PER_SWEEP: usize = 200;

    for note in s.state.needing_parse(grammar, PER_SWEEP)? {
        let Some(text) = note.text.as_deref() else {
            continue;
        };
        let zone = note
            .tz
            .as_deref()
            .and_then(Zone::new)
            // A capture with no zone recorded is read in UTC rather than in
            // whatever this machine happens to be set to. Reading it locally
            // would make the same note parse differently on two devices, which
            // is the property the whole parse path is arranged to avoid.
            .unwrap_or_else(|| Zone::new("UTC").expect("UTC is always present"));

        let r = brain_parse::recognize(text, now, zone);
        let Some(as_) = promoted_from(&r, zone, now) else {
            continue;
        };

        s.append(Kind::PROMOTE, Some(note.tgt), None, |nonce| {
            encode(&PromoteBody {
                nonce,
                as_: as_.clone(),
                pinned: false,
                grammar: Some(grammar),
                confidence: Some(r.confidence),
            })
        })?;
        written += 1;
    }

    Ok(written)
}

/// Assign a type by hand, and pin it.
///
/// A promotion authored this way is never displaced by a later parse, however
/// much the grammar improves. That is the one conflict rule in the system that
/// is not plain last-write-wins, and it exists because a reparse silently
/// reverting somebody's correction is the failure §6 rates unrecoverable.
pub fn correct(paths: &StorePaths, name: &str, as_: Promoted) -> Result<()> {
    let mut s = Session::open(paths.clone())?;
    let tgt = crate::mutate::find(&s, name)?;
    s.append(Kind::PROMOTE, Some(tgt), None, |nonce| {
        encode(&PromoteBody {
            nonce,
            as_: as_.clone(),
            pinned: true,
            // No grammar and no confidence: a person did this, and recording a
            // version here would make the sweep think a parser had.
            grammar: None,
            confidence: None,
        })
    })?;
    Ok(())
}

/// Change a series from one occurrence onward.
///
/// Bounds the existing rule with `UNTIL` immediately before the occurrence,
/// captures a new note carrying the same text, and promotes it to a series
/// starting at that occurrence with `split_from` naming the original (§3.5).
///
/// # Why this is not an edit
///
/// Editing the rule in place would change what the schedule *was* last March.
/// For an ordinary calendar that is a wrong entry; here it is worse, because a
/// log that quietly revises its own history is an unreliable narrator, and the
/// whole purpose of the system is compensating for unreliable memory. Splitting
/// leaves the past exactly as it happened.
///
/// The two other scopes are already available and are not this: changing one
/// occurrence is an `occ_*` exception, and changing every occurrence is an
/// ordinary edit of the rule.
pub fn split_series(paths: &StorePaths, name: &str, at_occ: i64, new_rule: RuleSpec) -> Result<()> {
    let (tgt, current, text, tz) = {
        let s = Session::open(paths.clone())?;
        let tgt = crate::mutate::find(&s, name)?;
        let all = s.log.all()?;
        let folded = brain_core::fold::fold(&all);
        let note = folded
            .notes
            .get(&tgt)
            .ok_or_else(|| crate::Error::NotFound(name.into()))?;

        let Some(brain_core::fold::Promotion {
            as_: Promoted::Series(sf),
            ..
        }) = note.promotion.clone()
        else {
            return Err(crate::Error::Usage(format!(
                "'{name}' is not a recurring series, so there is nothing to split"
            )));
        };
        (
            tgt,
            sf,
            note.text.clone().unwrap_or_default(),
            note.tz.clone(),
        )
    };

    // 1. Bound the old rule. Pinned, because a person asked for this and no
    //    reparse may undo it.
    let mut bounded = current.clone();
    bounded.rule.until = Some(at_occ - 1);
    correct(paths, name, Promoted::Series(bounded))?;

    // 2. The new series is its own note. It carries the same text because it is
    //    the same commitment continuing under a different schedule — and the
    //    text is what a person recognises it by.
    let mut s = Session::open(paths.clone())?;
    let body = crate::session::capture_body_in(&text, tz.as_deref())?;
    let new_tgt = s.capture(&body)?;
    let new_tgt = crate::mutate::find(&s, &new_tgt.slug)?;

    let when = match &current.when {
        BodyWhen::Timed { end_ms, tz, .. } => BodyWhen::Timed {
            start_ms: at_occ,
            end_ms: *end_ms,
            tz: tz.clone(),
        },
        all_day => all_day.clone(),
    };

    s.append(Kind::PROMOTE, Some(new_tgt), None, |nonce| {
        encode(&PromoteBody {
            nonce,
            as_: Promoted::Series(SeriesFields {
                when: when.clone(),
                rule: new_rule.clone(),
                carries_forward: current.carries_forward,
                split_from: Some(tgt),
            }),
            pinned: true,
            grammar: None,
            confidence: None,
        })
    })?;

    Ok(())
}

/// A task, as the correction path builds one.
pub fn task(done: bool, due: Option<&str>) -> Promoted {
    Promoted::Task(TaskFields {
        done,
        due: due.map(String::from),
        order: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use brain_parse::recognize;

    /// Monday 1 June 2026, 09:00 in Denver.
    const NOW: i64 = 1_780_326_000_000;

    fn denver() -> Zone {
        Zone::new("America/Denver").unwrap()
    }

    fn read(text: &str) -> Recognition {
        recognize(text, NOW, denver())
    }

    fn promoted_from_at(r: &Recognition, zone: Zone) -> Option<Promoted> {
        promoted_from(r, zone, NOW)
    }

    #[test]
    fn the_floor_is_the_one_the_design_states() {
        assert_eq!(FLOOR, 75);
    }

    #[test]
    fn a_confident_reading_becomes_an_event() {
        let p = promoted_from_at(&read("dentist thursday 2pm"), denver());
        assert!(matches!(p, Some(Promoted::Event(_))), "{p:?}");
    }

    #[test]
    fn a_reading_under_the_floor_writes_nothing() {
        // Bare weekday, vague time, task verb — all suggestions, none of them
        // types. This is the whole difference between a glance and a calendar
        // entry.
        for text in [
            "dentist thursday",
            "call mum tomorrow morning",
            "send sarah the migration doc",
        ] {
            assert!(
                promoted_from_at(&read(text), denver()).is_none(),
                "{text:?} promoted from under the floor"
            );
        }
    }

    #[test]
    fn a_range_becomes_all_day_dates_and_never_an_instant() {
        // The most common calendar bug in the wild, refused at the point where
        // it would be introduced.
        match promoted_from_at(&read("vacation aug 1-14"), denver()) {
            Some(Promoted::Event(f)) => match f.when {
                BodyWhen::AllDay {
                    start_date,
                    end_date,
                } => {
                    assert_eq!(start_date, "2026-08-01");
                    assert_eq!(end_date, "2026-08-14");
                }
                other => panic!("a range became {other:?}"),
            },
            other => panic!("vacation aug 1-14 -> {other:?}"),
        }
    }

    #[test]
    fn a_recognised_series_never_carries_forward() {
        // §3.6: the recognizer must not infer that a series accumulates.
        // "walk the dog daily" and "take the pill daily" are the same sentence
        // to a parser and opposite answers to a person.
        match promoted_from_at(&read("walk the dog every day at 7"), denver()) {
            Some(Promoted::Series(s)) => assert!(!s.carries_forward),
            other => panic!("expected a series, got {other:?}"),
        }
    }

    #[test]
    fn a_duration_is_used_but_never_invented() {
        // An end time nobody wrote is harder to notice than a missing one.
        match promoted_from_at(&read("standup thursday 9am"), denver()) {
            Some(Promoted::Event(f)) => match f.when {
                BodyWhen::Timed { end_ms, .. } => assert_eq!(end_ms, None),
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }

        match promoted_from_at(&read("standup thursday 9am ~30m"), denver()) {
            Some(Promoted::Event(f)) => match f.when {
                BodyWhen::Timed {
                    start_ms, end_ms, ..
                } => {
                    assert_eq!(end_ms, Some(start_ms + 30 * 60_000));
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    /// A real folded note, so these test the shape the sweep actually sees.
    fn a_note() -> Note {
        let s = brain_core::testing::mixed(5, 1, 1, 0);
        brain_core::fold::fold(&s.events)
            .notes
            .into_values()
            .next()
            .expect("the scenario has one note")
    }

    fn promotion(pinned: bool, grammar: Option<u32>, n: &Note) -> brain_core::fold::Promotion {
        brain_core::fold::Promotion {
            as_: Promoted::Note,
            pinned,
            grammar,
            confidence: grammar.map(|_| 90),
            at: n.created,
            event: n.tgt,
        }
    }

    #[test]
    fn a_pinned_promotion_is_never_reparsed() {
        // The pin is what makes a correction stick. A sweep that reparsed it
        // would silently undo somebody's fix the next time the grammar moved.
        let mut note = a_note();
        note.promotion = Some(promotion(true, None, &note.clone()));
        assert!(!needs_parse(&note, 99));
    }

    #[test]
    fn an_older_grammar_is_read_again() {
        let mut note = a_note();
        note.promotion = Some(promotion(false, Some(1), &note.clone()));
        assert!(needs_parse(&note, 2), "a grammar fix must reach old notes");
        assert!(!needs_parse(&note, 1), "nothing changed, nothing to redo");
    }

    #[test]
    fn a_note_nothing_has_read_yet_needs_reading() {
        assert!(needs_parse(&a_note(), 1));
    }
}
