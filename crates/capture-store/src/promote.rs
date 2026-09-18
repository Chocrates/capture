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

use capture_core::body::{
    EventFields, PromoteBody, Promoted, RuleSpec, SeriesFields, TaskFields, When as BodyWhen,
};
use capture_core::event::Kind;
use capture_core::fold::Note;
use capture_parse::{CONFIDENCE_FLOOR, Recognition, Shape, When};
use capture_time::Zone;

use crate::session::{Session, encode};
use crate::{Error, Result, StorePaths};

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
        Shape::Nothing => None,

        // A verb with nothing else stays a suggestion. It is below the floor
        // and `clears_floor` has already refused it; the arm is here so that
        // adding a band later cannot promote one by accident.
        Shape::Task { due: None } => None,

        // Something to do, due on a day. A floating date and never an instant:
        // "due Friday" is not 00:00 UTC on Friday, and §3.6 makes that the
        // difference between a due date and an appointment nobody made.
        Shape::Task { due: Some(d) } => Some(Promoted::Task(TaskFields {
            done: false,
            due: Some(format!("{d}")),
            order: None,
        })),

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
    let today = capture_time::civil_date(now, zone);
    match at_minute {
        Some(minutes) => Some(BodyWhen::Timed {
            start_ms: capture_time::at_local(today, minutes / 60, minutes % 60, zone)?,
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
        // Never read, so read it.
        None => true,
        // Somebody's correction. §6 rates a reparse silently displacing one as
        // the failure worth preventing above all others here.
        Some(p) if p.pinned => false,
        Some(p) => {
            // A better grammar than the one that read it.
            let older_grammar = p.grammar.is_some_and(|g| g < grammar);

            // Or the text has changed since it was read.
            //
            // This was missing, and the milestone that recorded it as done
            // shipped without it. Editing "dentist thursday 2pm" into "dentist
            // friday 3pm" left the promotion saying Thursday at two: the note
            // said one thing, the agenda showed another, and nothing reparsed
            // because nothing was watching the text.
            //
            // §3.1.4 keeps text and fields apart deliberately and shows both,
            // so *deliberate* drift is visible. An edit that silently did not
            // take is not that — it is the fields going stale, which is the
            // thing that separation was supposed to make impossible to miss.
            //
            // `updated` moves only on a capture or an edit; a promotion is not
            // a version and does not touch it. So this settles instead of
            // re-firing on the promotion it just wrote.
            let older_than_the_text = p.at < note.updated;

            older_grammar || older_than_the_text
        }
    }
}

/// Read everything that needs reading, and write what clears the floor.
///
/// Returns how many promotions were written. Runs on a session that is already
/// open, so it costs nothing when there is nothing to do.
///
/// Takes no clock, deliberately. Every note is read against the moment it was
/// captured, so there is no "now" for a caller to supply — and the parameter
/// that used to be here was the bug: it made a reparse reschedule what it read.
pub fn sweep(s: &mut Session) -> Result<usize> {
    let grammar = capture_parse::GRAMMAR_VERSION;
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

        // Read against the moment it was captured, never against now.
        //
        // Caught on a live store the first time a grammar bump reparsed one:
        // "meeting at 14:00", captured yesterday, came back resolved to *today*
        // — and would have moved again on every future bump. A sweep is
        // supposed to be a better reading of what somebody wrote, not a
        // rescheduling of it, and "at 14:00" written on Tuesday means Tuesday
        // however many times it is read afterwards.
        //
        // This is the same property the recognizer is arranged around: it is a
        // pure function of `(text, now, zone)`, and for a given note the only
        // honest `now` is when it was written. It also makes the first parse
        // and every reparse agree, which they did not before — enrichment runs
        // seconds after capture, so the difference was invisible until enough
        // time passed for it to matter.
        let captured = note.created.0 as i64;

        let r = capture_parse::recognize(text, captured, zone);
        let as_ = match promoted_from(&r, zone, captured) {
            Some(as_) => as_,

            // Nothing temporal in it any more, and something used to be.
            //
            // A reparse could promote and never demote, so editing "budget 40k
            // every friday" down to "budget 40k" left the series standing: an
            // agenda entry whose own text no longer says a time, which is the
            // staleness the reparse exists to prevent, in the other direction.
            //
            // Only where a promotion is already there. A note that has never
            // been anything stays nothing — writing `note` for every ordinary
            // capture would put an event in the log for each one to record that
            // the recognizer found what it usually finds.
            // One indexed read per note the sweep was already going to write,
            // which is bounded and rare.
            None if s.state.promotion_of(&note.tgt)?.is_some() => Promoted::Note,

            None => continue,
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

/// Assign a task by hand, with an optional due date.
///
/// The hand-correction path for the type the recognizer is most cautious
/// about. A verb alone stays a suggestion forever — verbs are an open class and
/// any list is wrong at the edges — so there has to be a way to say "yes, this
/// one" without editing the text to trick the parser into agreeing.
///
/// The due date is a floating date and is parsed as one, so a malformed date is
/// refused here rather than stored and discovered later by a query that
/// silently matches nothing.
pub fn as_task(paths: &StorePaths, name: &str, due: Option<&str>) -> Result<()> {
    let due = match due {
        None => None,
        Some(d) => {
            let parsed = capture_time::FloatingDate::parse(d).ok_or_else(|| {
                crate::Error::Usage(format!("'{d}' is not a date; use YYYY-MM-DD"))
            })?;
            // Normalised rather than echoed, so `2026-6-1` and `2026-06-01` are
            // one value in the projection and a query for one finds the other.
            Some(alloc_string(parsed))
        }
    };

    correct(
        paths,
        name,
        Promoted::Task(TaskFields {
            done: false,
            due,
            order: None,
        }),
    )
}

fn alloc_string(d: capture_time::FloatingDate) -> String {
    format!("{d}")
}

/// Mark a task done, or open again.
///
/// Reads the current fields rather than replacing them, because a completion
/// must not silently discard a due date somebody set. Undoing is the same
/// operation with the flag the other way — §6 rates a state that cannot be
/// reversed from any device as the failure worth avoiding, and a mistaken tick
/// is the ordinary case rather than the exotic one.
pub fn set_done(paths: &StorePaths, name: &str, done: bool) -> Result<()> {
    let current = {
        let store = crate::projection::Store::open_read(&crate::projection::generation_path(
            &paths.state,
            crate::ops::Ops::open_read(&paths.ops)?.generation()?,
        ))?;
        let tgt = store
            .resolve(name)?
            .ok_or_else(|| crate::Error::NotFound(name.to_string()))?;
        store
            .promotion_of(&tgt)?
            .map(|p| p.as_)
            .ok_or_else(|| crate::Error::Usage(format!("'{name}' is not a task")))?
    };

    let Promoted::Task(mut fields) = current else {
        return Err(crate::Error::Usage(format!(
            "'{name}' is not a task; a series occurrence is completed with its own date"
        )));
    };

    fields.done = done;
    correct(paths, name, Promoted::Task(fields))
}

/// Say that missing an instance of a series leaves something owed.
///
/// Only ever set by a person. §3.6 is explicit that the recognizer must not
/// infer this: "walk the dog daily" and "take the pill daily" are the same
/// sentence to a parser and opposite answers to somebody living with the
/// consequences, and guessing wrong in the direction of a manufactured backlog
/// is the todo-app failure the default exists to avoid.
///
/// Lives here rather than in a surface because there are now two surfaces
/// asking for it — the command line and the review pane — and the rule that it
/// is only meaningful for a series is part of the operation rather than part of
/// either one's presentation.
pub fn carries_forward(paths: &StorePaths, name: &str) -> Result<()> {
    let current = {
        let store = crate::projection::Store::open_read(&crate::projection::generation_path(
            &paths.state,
            crate::ops::Ops::open_read(&paths.ops)?.generation()?,
        ))?;
        let tgt = store
            .resolve(name)?
            .ok_or_else(|| crate::Error::NotFound(name.to_string()))?;
        store.promotion_of(&tgt)?.map(|p| p.as_).ok_or_else(|| {
            crate::Error::Usage(format!("'{name}' has not been recognised as anything"))
        })?
    };

    let Promoted::Series(mut fields) = current else {
        return Err(crate::Error::Usage(format!(
            "'{name}' is not a recurring series, so nothing about it can be owed"
        )));
    };

    fields.carries_forward = true;
    correct(paths, name, Promoted::Series(fields))
}

/// Change a series from one occurrence onward, named the way a person would.
///
/// The store's half of `capture split` and of the window's "Change from…". Both
/// surfaces had a copy of this — parse a date, parse a rule, find the
/// boundary, call [`split_series`] — which is two sets of rules about what a
/// date means and which occurrence it picks. The second set is the one that
/// drifts, because it is the one exercised least.
///
/// `now` is supplied rather than read so the rule is interpreted against a
/// stated moment, which is what makes the same phrase mean the same thing
/// here as it does at capture.
pub fn split_at(
    paths: &StorePaths,
    name: &str,
    from: &str,
    rule_text: &str,
    now: i64,
    zone: &str,
) -> Result<Split> {
    // The rule first, before the store is opened. A rule that is not a rule is
    // wrong whatever the note's occurrences look like, and reporting it as
    // "nothing after that point" sends the reader to the wrong problem.
    let zone = Zone::new(zone)
        .or_else(|| Zone::new("UTC"))
        .ok_or_else(|| Error::Usage("no usable time zone on this machine".into()))?;

    let read = capture_parse::recognize(rule_text, now, zone);
    let Shape::Recurring { rule, .. } = read.shape else {
        return Err(Error::Usage(format!(
            "{rule_text:?} is not a repeating rule — try something like \"every tuesday at 9\""
        )));
    };

    let day = local_midnight(from)?;

    let (boundary, before) = {
        let s = Session::open(paths.clone())?;
        let tgt = s
            .state
            .resolve(name)?
            .ok_or_else(|| Error::NotFound(name.to_string()))?;
        let occurrences = s.state.occurrences_of(&tgt)?;
        if occurrences.is_empty() {
            return Err(Error::Usage(format!("{name} is not a recurring series")));
        }

        // The first occurrence on or after the day named, not an exact match:
        // a person says "from September" and means the next one, not one that
        // has to fall on a particular instant.
        let at = occurrences.iter().find(|o| o.occ >= day).ok_or_else(|| {
            Error::Usage(format!(
                "{name} has no occurrence on or after {from} — nothing after that point to change"
            ))
        })?;
        let boundary = at.occ;
        (
            boundary,
            occurrences.iter().filter(|o| o.occ < boundary).count(),
        )
    };

    split_series(paths, name, boundary, rule)?;
    Ok(Split { boundary, before })
}

/// What a split did.
///
/// Deliberately without a count of what follows. The occurrences after the
/// boundary were produced by the *old* rule, so reporting their number would
/// state how often the new rule fires — a different question, with a different
/// answer, that this has not asked.
pub struct Split {
    /// The occurrence the change starts at.
    pub boundary: i64,
    /// How many occurrences before it keep the rule they had.
    pub before: usize,
}

/// A `YYYY-MM-DD` date at local midnight.
///
/// Deliberately not the full recognizer. "Next tuesday" is fine for capturing
/// something and wrong for naming a boundary in a schedule that will outlive
/// the sentence: the person reading it back in a year needs to know which day
/// was meant.
fn local_midnight(s: &str) -> Result<i64> {
    let date: jiff::civil::Date = s
        .parse()
        .map_err(|_| Error::Usage(format!("{s:?} is not a date — write it as 2026-09-15")))?;
    date.to_zoned(jiff::tz::TimeZone::system())
        .map(|z| z.timestamp().as_millisecond())
        .map_err(|_| Error::Usage(format!("{s:?} is not a date that exists")))
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
        let folded = capture_core::fold::fold(&all);
        let note = folded
            .notes
            .get(&tgt)
            .ok_or_else(|| crate::Error::NotFound(name.into()))?;

        let Some(capture_core::fold::Promotion {
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
    use capture_parse::recognize;

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
        let s = capture_core::testing::mixed(5, 1, 1, 0);
        capture_core::fold::fold(&s.events)
            .notes
            .into_values()
            .next()
            .expect("the scenario has one note")
    }

    fn promotion(pinned: bool, grammar: Option<u32>, n: &Note) -> capture_core::fold::Promotion {
        capture_core::fold::Promotion {
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

/// What ticking an item did, so a surface can say it rather than guess.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ticked {
    /// The item became a task and carries the state asked for.
    Done,
    /// It was already in that state. Said separately rather than reported as
    /// success, so a surface can stay quiet instead of claiming it acted.
    Already,
    /// More than one item on this list reads the same, and completion matches
    /// by text. Carries how many, because "two" and "nine" want different
    /// words from whoever has to explain it.
    Ambiguous(usize),
    /// Nothing on this list reads like that.
    NoSuchItem,
}

/// Tick an item off a list, or put it back.
///
/// # Why this appends an event and mints nothing
///
/// An item is a projection row expanded from its list capture's text, exactly
/// as an occurrence is expanded from a recurring rule. Completing one is a
/// fact *about* that capture, so it is recorded the way completing an
/// occurrence is: an event naming what it is about, and nothing new in the
/// archive.
///
/// The first version of this promoted the item to a task instead, which meant
/// ticking off the shopping created a capture called `milk` that nobody wrote.
/// A week of it would have filled the timeline with groceries.
///
/// # Why the item is named by its text
///
/// Positions shift. Editing `#groceries: milk, eggs` into `#groceries: oat
/// milk, eggs` moves everything after the edit, and a tick bound to an index
/// would move with it — silently, onto a different item. Naming by text is the
/// same choice `OccBody` makes in naming an occurrence by the instant its rule
/// would have produced rather than by its index (§3.5).
///
/// The cost is real and is refused rather than guessed at: two items on one
/// list reading the same text cannot be told apart.
pub fn tick(paths: &StorePaths, tag: &str, item: &str, done: bool, now: u64) -> Result<Ticked> {
    let item = item.trim();

    // What the list knows, read before anything is written. Both answers below
    // are refusals, and a refusal that had already appended an event would
    // leave a record of something that did not happen.
    let (on_list, target, already) = {
        let store = reading(paths)?;
        let count = store.list_item_count(tag, item)?;
        let found = store.list(tag, now)?.into_iter().find(|e| e.text == item);
        (count, found.as_ref().map(|e| e.tgt), found.map(|e| e.done))
    };

    if on_list > 1 {
        return Ok(Ticked::Ambiguous(on_list));
    }
    let (Some(tgt), Some(was)) = (target, already) else {
        return Ok(Ticked::NoSuchItem);
    };
    if was == done {
        return Ok(Ticked::Already);
    }

    let mut s = crate::session::Session::open(paths.clone())?;
    let kind = if done {
        capture_core::event::Kind::ITEM_DONE
    } else {
        capture_core::event::Kind::ITEM_UNDONE
    };
    s.append(kind, Some(tgt), None, |nonce| {
        minicbor::to_vec(capture_core::body::ItemBody {
            nonce,
            item: item.to_string(),
        })
        .expect("encoding into a Vec is infallible")
    })?;

    Ok(Ticked::Done)
}

/// The current generation, read-only. The same three lines `set_done` opens
/// with, named once because `tick` needs them twice.
fn reading(paths: &StorePaths) -> Result<crate::projection::Store> {
    crate::projection::Store::open_read(&crate::projection::generation_path(
        &paths.state,
        crate::ops::Ops::open_read(&paths.ops)?.generation()?,
    ))
}
