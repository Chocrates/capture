//! The recognizer: three tiers, in order, and the guards that keep tier 2 off
//! ordinary prose.
//!
//! # It returns a recognition, never an event
//!
//! This is a pure function of `(text, now, zone)`. It decides what a capture
//! looks like and how sure it is; it does not decide whether that clears the
//! floor, whether a human correction already governs, or whether to author
//! anything. Those need the event log, and keeping them out means this can be
//! tested exhaustively over the closed vocabulary with no store in sight — and
//! that the confidence floor is enforced in exactly one place rather than at
//! every call site.
//!
//! # The guards matter more than the grammar
//!
//! A context-free grammar over this vocabulary fires constantly on ordinary
//! prose, and §3.1.3 is explicit that false positives are the worse error: a
//! wrong suggestion costs a glance, a wrong promotion costs a calendar entry.
//! So [`guarded`] is applied to every candidate match, and the document's own
//! examples — "the march timeline slipped", "the 7am problem", "budget 40k" —
//! are tests rather than reasoning.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use capture_core::body::RuleSpec;
use capture_time::{FloatingDate, Zone};

use crate::{Class, Token, tokenize};

/// The explicit markers found in a capture (§3.1.1).
///
/// Always collected, whatever tier 2 makes of the rest. A sigil is exact and
/// beats inference, so these are gathered first and the grammar is told which
/// spans to leave alone.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Markers {
    /// `#tag`.
    pub tags: Vec<String>,
    /// `@person`.
    pub people: Vec<String>,
    /// `>date`, exactly as written.
    pub date: Option<String>,
    /// `!p1` — the digit.
    pub priority: Option<u32>,
    /// `~30m`, in minutes.
    pub duration_min: Option<u32>,
    /// `^project`.
    pub project: Option<String>,
}

impl Markers {
    /// Whether any explicit marker at all was written.
    pub fn any(&self) -> bool {
        !self.tags.is_empty()
            || !self.people.is_empty()
            || self.date.is_some()
            || self.priority.is_some()
            || self.duration_min.is_some()
            || self.project.is_some()
    }
}

/// When something was recognised as happening.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum When {
    /// A day, with no time of day. A floating date, never an instant.
    Date(FloatingDate),
    /// A moment: UTC milliseconds, resolved in the capture's zone.
    Moment(i64),
    /// A day and a sun event on it: "tomorrow 30 min before sunset". No
    /// instant, because none exists until somewhere is known (D2).
    Sun(FloatingDate, Sun),
}

/// Sunrise or sunset, and how far from it (alarms-at-sunrise-and-sunset D8).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Sun {
    /// [`capture_core::body::SUN_RISE`] or [`capture_core::body::SUN_SET`].
    pub event: u8,
    /// Minutes after it; negative is before.
    pub offset_min: i16,
}

impl Sun {
    /// The furthest from the sun a reading promotes on its own. Beyond it the
    /// reading is offered rather than written: "7 hours after sunset" is more
    /// likely a slip than an alarm, and a person can still accept it.
    pub const NEAR_MIN: i16 = 6 * 60;

    /// Whether the offset is past [`Sun::NEAR_MIN`].
    pub fn is_far(self) -> bool {
        self.offset_min.unsigned_abs() > Self::NEAR_MIN as u16
    }
}

/// What a capture was recognised as.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Shape {
    /// Nothing temporal, and no reason to think it is a task.
    Nothing,
    /// A task verb at the head: something to do, which occupies no time.
    ///
    /// §3.6's table is the discriminator, and it is about time rather than
    /// wording: an event *occupies* time and has no state, a task has state and
    /// occupies none. So a clock makes an event however the sentence starts —
    /// "call sarah at 3pm" is a call you are on at three — and a bare day
    /// behind a task verb makes a task that is *due* that day. "Due Friday" is
    /// deliberately a floating date and never an instant, because 00:00 UTC on
    /// Friday is a different claim (§3.6).
    Task {
        /// The day it is due, where one was written.
        due: Option<FloatingDate>,
    },
    /// One point in time.
    Point(When),
    /// A span of days.
    Range {
        /// First day, inclusive.
        from: FloatingDate,
        /// Last day, inclusive.
        to: FloatingDate,
    },
    /// A repeating rule, and the time of day it happens at.
    Recurring {
        /// The rule itself.
        rule: RuleSpec,
        /// Minutes past local midnight, where one was given.
        at_minute: Option<u32>,
    },
    /// A repeating rule at a sun event: "every day at sunrise". Its own shape
    /// rather than a field beside `at_minute`, because a series has one or the
    /// other and never both.
    SunRecurring {
        /// The rule itself.
        rule: RuleSpec,
        /// Which sun event, and the offset from it.
        sun: Sun,
    },
}

/// What the recognizer made of a capture.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Recognition {
    /// The shape.
    pub shape: Shape,
    /// Confidence in hundredths, to compare against [`crate::CONFIDENCE_FLOOR`].
    pub confidence: u32,
    /// Explicit markers, which are exact and never scored.
    pub markers: Markers,
    /// The grammar this was produced by.
    pub grammar: u32,
    /// A bare hour that could have been either half of the clock.
    pub unclear_hour: Option<UnclearHour>,
    /// A day the words leave two answers for.
    pub unclear_day: Option<UnclearDay>,
}

/// An hour written without am or pm, which the recognizer will not guess.
///
/// "gym every monday at 2" was scheduled for 02:00, and "dentist on oct 15th at
/// 2" with it — found in use on 2026-10-12. A guess that is right most days is
/// the failure noticed only when it matters, at 2 am on the day; the person
/// chose being asked. So the date or the rule promotes without a time, and this
/// records what to ask and where the answer goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnclearHour {
    /// The hour as written, 1 to 12.
    pub hour: u32,
    /// Byte offset just past the number, where `am` or `pm` would go.
    pub at: usize,
}

/// A day the words leave two answers for, which the recognizer will not pick.
///
/// Found in use on 2026-10-13. "therapy next thursday at 4pm", written on a
/// Tuesday, was put on the 15th when the 22nd was meant; corrected to "therapy
/// Thursday 2026-10-22 at 4pm", it was put on the 15th again, because a weekday
/// in front of a date was read alone. Two readings arise:
///
/// - "next X" while this week's X is still ahead: this week's, or next week's.
/// - A weekday beside a date it is not: the X nearest the date, or the date.
///
/// Neither is guessed, for the reason the bare hour is not (grammar 7): a guess
/// right most of the time is noticed only on the day it is wrong. Nothing is
/// promoted, and this records the two days and the words to replace.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UnclearDay {
    /// The two days, earlier first.
    pub choices: [FloatingDate; 2],
    /// Byte offset of the words that named the day.
    pub start: usize,
    /// Byte offset just past them.
    pub end: usize,
}

/// The day a capture leaves open between two, read as the recognizer reads it.
///
/// Depends on the moment and zone it was written in, unlike [`crate::unclear_hour`]:
/// whether "next thursday" is this week's depends on which day it was said.
pub fn unclear_day(text: &str, now: i64, zone: Zone) -> Option<UnclearDay> {
    recognize(text, now, zone).unclear_day
}

/// The text with its unclear day answered: the words that named the day
/// replaced by ones that read back as choice `which` (0 or 1).
///
/// "therapy next thursday at 4pm" becomes "therapy thursday oct 22 at 4pm".
/// The weekday is kept so the text still reads as written, and a weekday beside
/// a date that agrees with it is one day (grammar 9).
pub fn answer_day(text: &str, now: i64, zone: Zone, which: usize) -> Option<alloc::string::String> {
    let u = unclear_day(text, now, zone)?;
    use alloc::format;
    let day = *u.choices.get(which)?;
    Some(format!(
        "{}{}{}",
        &text[..u.start],
        day_in_words(day, now, zone)?,
        &text[u.end..]
    ))
}

/// `thursday oct 22`, or `thursday 2026-10-22` where the shorter form would
/// be read as another year from `now`.
pub fn day_in_words(day: FloatingDate, now: i64, zone: Zone) -> Option<alloc::string::String> {
    use alloc::format;
    const DAYS: [&str; 7] = [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ];
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let weekday = DAYS[day.weekday()? as usize];
    Some(if year_for(now, zone, day.month, day.day) == day.year {
        format!("{weekday} {} {}", MONTHS[day.month as usize - 1], day.day)
    } else {
        format!("{weekday} {day}")
    })
}

/// What kind of anchor a match found, which is what its band depends on.
///
/// §3.1.5 scores by shape rather than by whether a marker happened to be
/// present: "in 3 weeks" is listed at 0.85 as two adjacent vocabulary items,
/// even though the `in` that opens it is a marker. Reading the marker alone
/// would score it 0.90, which is the band reserved for a complete date.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Anchor {
    /// A month and a day, or an explicit date.
    CompleteDate,
    /// A weekday or a day word — "thursday", "tomorrow".
    Day,
    /// A relative offset — "in 3 weeks", "next month", "Q2".
    Offset,
}

/// Confidence bands, in hundredths (§3.1.5).
///
/// # Provisional, and strict
///
/// These were reasoned rather than measured: they were written before a parser
/// existed and before there was a corpus to check them against. They decide
/// what silently becomes a calendar entry and what waits as a suggestion, so
/// being wrong in the generous direction produces exactly the false positives
/// §3.1.3 calls the worse error.
///
/// So they are treated as strict. **A band moves only when a real capture
/// forces it, and that capture is pinned alongside as a test case.** The bands
/// accumulate evidence in the direction the design already prefers — a miss
/// costs a glance at a suggestion, a false positive costs a calendar entry —
/// and every loosening leaves a record of what demanded it.
mod band {
    /// A complete date or datetime behind a marker.
    pub const MARKED: u32 = 90;
    /// Two vocabulary items adjacent and unambiguous.
    pub const ADJACENT: u32 = 85;
    /// A recurrence head with a body.
    pub const RECURRENCE: u32 = 80;
    /// A bare weekday or day word with no clock.
    pub const BARE_DAY: u32 = 70;
    /// A time-of-day word standing in for a clock.
    pub const VAGUE: u32 = 60;
    /// A task verb and a bare day: `call the dentist tomorrow`.
    ///
    /// A new band rather than a moved one. Nothing scored this shape before,
    /// because the shapes did not compose: a task verb in front of a date was
    /// read as a bare date and the verb was thrown away, so `call the dentist
    /// tomorrow` became an all-day event and `finish the deck by friday` became
    /// one too. Both are things to do, and neither occupies a Friday.
    ///
    /// Above the floor, which is a decision and argued rather than nudged.
    /// §3.1.3 prices a false positive as a wrongly created *calendar entry* —
    /// something with a time, on a grid, that a notifier can fire. A wrongly
    /// created task has none of those: no time, no grid position, nothing that
    /// fires, and `capture done` or an ordinary correction removes it. The
    /// evidence is also two independent signals agreeing rather than one — a
    /// verb at the head and a day — where the 0.70 band is a day alone.
    ///
    /// Kept below 0.85 all the same: two adjacent vocabulary items are a
    /// stronger claim about *what was written* than a verb plus a day, which is
    /// a claim about what was meant by it.
    pub const TASK_DUE: u32 = 80;
    /// A task verb and no temporal expression.
    pub const TASK_VERB: u32 = 40;
    /// A sun time more than six hours from the sun: offered, never written
    /// unasked (alarms-at-sunrise-and-sunset D8).
    pub const SUN_FAR: u32 = 50;
}

/// Read a capture.
///
/// `now` is the moment to resolve relative expressions against, and `zone` the
/// zone the capture was written in. Both are arguments rather than ambient, so
/// the same text read twice gives the same answer.
pub fn recognize(text: &str, now: i64, zone: Zone) -> Recognition {
    let tokens = tokenize(text);
    let markers = markers(&tokens);

    // A task verb at the head is a hint about what the sentence is *for*.
    // Verbs are an open class and any list of them is wrong at the edges, so it
    // is never the whole reading — but it does decide between two readings of
    // the same day, and that is what the composition below is.
    //
    // Computed before the sigil path, not after it, and that ordering was the
    // bug. A sigil returned immediately with a bare day, so the most precise
    // way to write a due date — an explicit marker and a full date — produced
    // an *event*: a thing you attend rather than a thing you owe, which is the
    // wrong half of §3.6's table. The more exactly somebody wrote it, the more
    // certainly it was misread.
    let head_is_verb = tokens
        .iter()
        .find(|t| t.class != Class::Break)
        .is_some_and(|t| t.class == Class::TaskVerb);

    // A sigil is exact and beats inference. Where one names a date, tier 2 is
    // not consulted for one at all — not even to record a competing suggestion
    // (§3.1.1).
    //
    // The verb is still read. These answer different questions: the sigil says
    // *which date*, the verb says *what kind of thing this is*, and nothing
    // about reading both makes an inference compete with a sigil. The date it
    // names is the date, at full confidence, either way.
    if let Some(v) = markers.date.as_deref()
        && let Some(d) = sigil_day(v, now, zone)
    {
        return Recognition {
            shape: if head_is_verb {
                Shape::Task { due: Some(d) }
            } else {
                Shape::Point(When::Date(d))
            },
            confidence: 100,
            markers,
            grammar: crate::GRAMMAR_VERSION,
            unclear_hour: None,
            unclear_day: None,
        };
    }

    // A marker that named nothing readable reaches here, and the grammar tier
    // is skipped for it. Falling through to the prose is what the first version
    // did, and it is the defect: `>asdf` beside the word "thursday" promoted
    // Thursday, confidently and silently. A marker is an instruction, and an
    // instruction that could not be followed is not permission to substitute a
    // different answer (§3.1.5).
    //
    // Skipped rather than consulted-and-filtered: the tier returns a shape and
    // a confidence together — a recurrence, a range, a point — and picking the
    // date back out of a shape afterwards is the kind of partial undo that goes
    // wrong the next time a shape is added.
    //
    // The capture is not refused and its text is untouched (§3.1, §3.1.3). It
    // simply carries no date, and a task verb still reads as a task with none —
    // below the floor, so it offers rather than asserts.
    let mut unclear_hour = None;
    let mut unclear_day = None;
    if markers.date.is_none()
        && let Some((shape, confidence)) = temporal(
            &tokens,
            now,
            zone,
            head_is_verb,
            &mut unclear_hour,
            &mut unclear_day,
        )
    {
        return Recognition {
            shape,
            confidence,
            markers,
            grammar: crate::GRAMMAR_VERSION,
            unclear_hour,
            unclear_day: None,
        };
    }

    // A day that could be either of two. Nothing about it is promoted, the
    // verb included: a task due on one of two days is a guess too.
    if unclear_day.is_some() {
        return Recognition {
            shape: Shape::Nothing,
            confidence: 0,
            markers,
            grammar: crate::GRAMMAR_VERSION,
            unclear_hour,
            unclear_day,
        };
    }

    // No temporal expression anywhere. The verb alone stays below the floor, so
    // it never promotes on its own — it exists so a surface can offer "task?"
    // as a suggestion and apply nothing.
    Recognition {
        shape: if head_is_verb {
            Shape::Task { due: None }
        } else {
            Shape::Nothing
        },
        confidence: if head_is_verb { band::TASK_VERB } else { 0 },
        markers,
        grammar: crate::GRAMMAR_VERSION,
        unclear_hour: None,
        unclear_day: None,
    }
}

/// The day a `>` marker names: a full date, a weekday, or a day word.
///
/// The published grammar has always said `">" ( iso_date | day_ref )`, and
/// `day_ref` existed nowhere else in the repo. Only the ISO spelling was read,
/// so every other one was recorded in `markers.date` and then dropped.
///
/// A weekday and a day word resolve through `next_weekday` and `day_word` —
/// the same two the grammar tier uses for a bare `friday` or `tomorrow`. That
/// is deliberate rather than convenient: two spellings of "which Friday is
/// Friday" is one decision made twice, and the second copy is free to drift
/// from the first.
///
/// Matched against `WEEKDAYS` by whole word, not through `weekday_bit`. That
/// helper's three-letter `starts_with` is safe where it is used, because the
/// scanner has already classified the token against the closed list; reached
/// straight from a marker value it reads `>satellite` as Saturday, `>monitor`
/// as Monday and `>sunset` as Sunday — the approximate matching of an unlisted
/// word that §3.1.5 forbids in as many words.
fn sigil_day(v: &str, now: i64, zone: Zone) -> Option<FloatingDate> {
    if let Some(d) = FloatingDate::parse(v) {
        return Some(d);
    }
    let word = v.to_ascii_lowercase();
    if crate::WEEKDAYS.iter().any(|k| k.word == word) {
        return next_weekday(now, zone, &word);
    }
    day_word(now, zone, &word)
}

/// Collect the explicit markers.
fn markers(tokens: &[Token<'_>]) -> Markers {
    let mut m = Markers::default();
    for t in tokens {
        let Class::Sigil(c) = t.class else { continue };
        let Some(v) = t.sigil_value() else { continue };
        match c {
            '#' => m.tags.push(v.to_string()),
            '@' => m.people.push(v.to_string()),
            '>' => m.date = Some(v.to_string()),
            '^' => m.project = Some(v.to_string()),
            '!' => {
                let digits = v.trim_start_matches(['p', 'P']);
                if let Ok(n) = digits.parse() {
                    m.priority = Some(n);
                }
            }
            '~' => m.duration_min = duration(v).or(m.duration_min),
            _ => {}
        }
    }
    m
}

/// `30m`, `2h`, `90` — the value after a `~`.
fn duration(v: &str) -> Option<u32> {
    let digits: String = v.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n: u32 = digits.parse().ok()?;
    let rest = v[digits.len()..].to_ascii_lowercase();
    Some(match rest.as_str() {
        "" | "m" | "min" | "mins" => n,
        "h" | "hr" | "hrs" => n * 60,
        "d" => n * 24 * 60,
        _ => return None,
    })
}

/// Tier 2, over the productions of §3.1.5.
fn temporal(
    tokens: &[Token<'_>],
    now: i64,
    zone: Zone,
    head_is_verb: bool,
    unclear: &mut Option<UnclearHour>,
    day: &mut Option<UnclearDay>,
) -> Option<(Shape, u32)> {
    // Recurrence first: `every weekday at 7` also contains a weekday, and the
    // longer reading is the one meant.
    if let Some(r) = recurrence(tokens, unclear) {
        return Some(r);
    }
    if let Some(r) = range(tokens, now, zone) {
        return Some(r);
    }
    // `head_is_verb` reaches only `point`, because it can only change one
    // reading: a day into a due date. A range is a span of days and a
    // recurrence is a rule, and neither becomes a task by being introduced by a
    // verb — "review the docs every friday" is still a recurring obligation.
    point(tokens, now, zone, head_is_verb, unclear, day)
}

/// `every weekday at 7`, `each tuesday`, `daily`.
fn recurrence(tokens: &[Token<'_>], unclear: &mut Option<UnclearHour>) -> Option<(Shape, u32)> {
    // `weekdays at sunrise`: a day set with no head, but only before a sun
    // phrase. A bare `weekdays` is not a recurrence anywhere else, and widening
    // that is a separate decision about every capture that says "weekdays".
    if let Some((d, sun)) = tokens.iter().enumerate().find_map(|(i, t)| {
        (t.class == Class::DaySet && guarded(tokens, t))
            .then(|| sun_at(tokens, i + 1).map(|(s, _)| (i, s)))
            .flatten()
    }) {
        let mut rule = RuleSpec {
            freq: 1,
            interval: 1,
            by_day: 0,
            by_month_day: Vec::new(),
            count: None,
            until: None,
        };
        rule.by_day = day_set_bits(tokens[d].text);
        return Some(sun_series(rule, sun));
    }

    let at = tokens
        .iter()
        .position(|t| matches!(t.class, Class::RecHead | Class::FreqWord) && guarded(tokens, t))?;
    let head = tokens[at];

    let mut rule = RuleSpec {
        freq: 0,
        interval: 1,
        by_day: 0,
        by_month_day: Vec::new(),
        count: None,
        until: None,
    };

    let mut i = at + 1;
    if head.class == Class::FreqWord {
        rule.freq = match head.text.to_ascii_lowercase().as_str() {
            "weekly" => 1,
            "monthly" => 2,
            "quarterly" => 2,
            "yearly" | "annually" => 3,
            // daily, nightly, hourly
            _ => 0,
        };
        if head.text.eq_ignore_ascii_case("quarterly") {
            rule.interval = 3;
        }
    } else {
        // `every other week`, `every 3 days`.
        while i < tokens.len() {
            match tokens[i].class {
                Class::Interval => {
                    rule.interval = 2;
                    i += 1;
                }
                Class::Number(n) if n > 0 => {
                    rule.interval = n;
                    i += 1;
                }
                _ => break,
            }
        }

        match tokens.get(i).map(|t| t.class) {
            Some(Class::DaySet) => {
                let word = tokens[i].text.to_ascii_lowercase();
                rule.freq = 1;
                rule.by_day = day_set_bits(&word);
                i += 1;
            }
            Some(Class::Weekday) => {
                rule.freq = 1;
                rule.by_day = weekday_bit(tokens[i].text)?;
                i += 1;
                // `every monday and thursday`
                while let Some(next) = tokens.get(i) {
                    let skip = next.text.eq_ignore_ascii_case("and") || next.class == Class::Break;
                    if skip {
                        i += 1;
                        continue;
                    }
                    let Some(bit) = weekday_bit(next.text) else {
                        break;
                    };
                    if next.class != Class::Weekday {
                        break;
                    }
                    rule.by_day |= bit;
                    i += 1;
                }
            }
            Some(Class::Unit) => {
                rule.freq = unit_freq(tokens[i].text)?;
                i += 1;
            }
            // `every` with nothing it can repeat over is not a recurrence.
            _ => return None,
        }
    }

    if let Some((sun, _)) = sun_at(tokens, i) {
        return Some(sun_series(rule, sun));
    }

    Some((
        Shape::Recurring {
            rule,
            at_minute: clock_after(tokens, i, unclear),
        },
        band::RECURRENCE,
    ))
}

fn sun_series(rule: RuleSpec, sun: Sun) -> (Shape, u32) {
    let band = if sun.is_far() {
        band::SUN_FAR
    } else {
        band::RECURRENCE
    };
    (Shape::SunRecurring { rule, sun }, band)
}

fn day_set_bits(word: &str) -> u8 {
    if word.to_ascii_lowercase().starts_with("weekend") {
        0b0110_0000 // Sat, Sun
    } else {
        0b0001_1111 // Mon..Fri
    }
}

/// A sun phrase starting at `i`, and how many tokens it took: `at sunset`, or
/// `30 min before sunrise`.
///
/// Never a bare sun word. "Watched the sunset" and "sunset boulevard" name the
/// sun rather than a time, and only `at`, `before` or `after` says otherwise.
/// Needs no guard: the phrase is distinctive on its own, and "walk the dog 30
/// min before sunset" puts it mid-clause, where people write it.
fn sun_at(tokens: &[Token<'_>], i: usize) -> Option<(Sun, usize)> {
    let t = tokens.get(i)?;
    if t.class == Class::Marker && t.text.eq_ignore_ascii_case("at") {
        let s = tokens.get(i + 1)?;
        let event = sun_event(s)?;
        return Some((
            Sun {
                event,
                offset_min: 0,
            },
            2,
        ));
    }
    let Class::Number(n) = t.class else {
        return None;
    };
    let per: u32 = match tokens.get(i + 1)?.text.to_ascii_lowercase().as_str() {
        "minute" | "minutes" | "min" | "mins" => 1,
        "hour" | "hours" | "hr" | "hrs" | "h" => 60,
        _ => return None,
    };
    let dir = tokens.get(i + 2)?;
    let sign: i16 = if dir.text.eq_ignore_ascii_case("before") {
        -1
    } else if dir.text.eq_ignore_ascii_case("after") {
        1
    } else {
        return None;
    };
    let event = sun_event(tokens.get(i + 3)?)?;
    // A day either way is the most a sun time can mean. Past it, the number
    // is something else and nothing is read.
    let minutes = n.checked_mul(per).filter(|m| *m <= 24 * 60)?;
    Some((
        Sun {
            event,
            offset_min: sign * minutes as i16,
        },
        4,
    ))
}

fn sun_event(t: &Token<'_>) -> Option<u8> {
    if t.class != Class::SunWord {
        return None;
    }
    match t.text.to_ascii_lowercase().as_str() {
        "sunrise" => Some(capture_core::body::SUN_RISE),
        "sunset" => Some(capture_core::body::SUN_SET),
        _ => None,
    }
}

/// A point at a sun event, with the day it is on.
fn sun_point(day: FloatingDate, sun: Sun) -> (Shape, u32) {
    let band = if sun.is_far() {
        band::SUN_FAR
    } else {
        band::ADJACENT
    };
    (Shape::Point(When::Sun(day, sun)), band)
}

/// A time of day appearing after position `i`, with or without a marker.
///
/// A bare number after a marker is an hour, but only `0` and `13`–`23` say
/// which one. `1`–`12` is recorded in `unclear` and is not a time: see
/// [`UnclearHour`].
fn clock_after(
    tokens: &[Token<'_>],
    mut i: usize,
    unclear: &mut Option<UnclearHour>,
) -> Option<u32> {
    while i < tokens.len() {
        match tokens[i].class {
            Class::Marker => i += 1,
            Class::Clock { hour, minute, .. } => return Some(hour * 60 + minute),
            Class::TodWord => return tod_minutes(tokens[i].text),
            // A bare number is an hour here only because a marker preceded it:
            // "every weekday at 7". Without the marker it stays a number.
            Class::Number(n) if n <= 23 && i > 0 && tokens[i - 1].class == Class::Marker => {
                if (1..=12).contains(&n) {
                    *unclear = Some(UnclearHour {
                        hour: n,
                        at: tokens[i].end,
                    });
                    return None;
                }
                return Some(n * 60);
            }
            _ => return None,
        }
    }
    None
}

fn tod_minutes(word: &str) -> Option<u32> {
    crate::TIME_WORDS
        .iter()
        .find(|w| w.word.eq_ignore_ascii_case(word))
        .and_then(|w| w.minutes)
}

fn weekday_bit(word: &str) -> Option<u8> {
    let w = word.to_ascii_lowercase();
    let idx = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"]
        .iter()
        .position(|p| w.starts_with(p))?;
    Some(1 << idx)
}

fn unit_freq(word: &str) -> Option<u8> {
    Some(match word.to_ascii_lowercase().as_str() {
        "day" | "days" => 0,
        "week" | "weeks" | "wk" | "wks" => 1,
        "month" | "months" | "mo" | "mos" => 2,
        "year" | "years" | "yr" | "yrs" => 3,
        _ => return None,
    })
}

/// `aug 1-14`.
fn range(tokens: &[Token<'_>], now: i64, zone: Zone) -> Option<(Shape, u32)> {
    let m = tokens
        .iter()
        .position(|t| t.class == Class::Month && guarded(tokens, t))?;

    let first = day_number(tokens.get(m + 1)?)?;
    if tokens.get(m + 2)?.class != Class::RangeOp {
        return None;
    }
    let second = day_number(tokens.get(m + 3)?)?;

    let month = month_number(tokens[m].text)?;
    let year = year_for(now, zone, month, first);

    Some((
        Shape::Range {
            from: FloatingDate::new(year, month, first)?,
            to: FloatingDate::new(year, month, second)?,
        },
        band::ADJACENT,
    ))
}

fn day_number(t: &Token<'_>) -> Option<u8> {
    match t.class {
        Class::Number(n) | Class::Ordinal(n) if (1..=31).contains(&n) => Some(n as u8),
        _ => None,
    }
}

fn month_number(word: &str) -> Option<u8> {
    let w = word.to_ascii_lowercase();
    let names = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    names
        .iter()
        .position(|p| w.starts_with(p))
        .map(|i| i as u8 + 1)
}

/// `dentist thursday 2pm`, `on aug 14`, `in 3 weeks`, `tomorrow morning`.
fn point(
    tokens: &[Token<'_>],
    now: i64,
    zone: Zone,
    head_is_verb: bool,
    unclear: &mut Option<UnclearHour>,
    unclear_day: &mut Option<UnclearDay>,
) -> Option<(Shape, u32)> {
    for (i, t) in tokens.iter().enumerate() {
        // `at sunset`, `30 min before sunrise tomorrow`: the sun first, then
        // the day after it if one is named, else today — as a bare clock is.
        if let Some((sun, used)) = sun_at(tokens, i) {
            let day = match tokens.get(i + used) {
                Some(d) if d.class == Class::DayWord => day_word(now, zone, d.text),
                Some(d) if d.class == Class::Weekday => next_weekday(now, zone, d.text),
                _ => None,
            }
            .unwrap_or(today(now, zone).1);
            return Some(sun_point(day, sun));
        }
        if !guarded(tokens, t) {
            continue;
        }
        let marked = i > 0 && tokens[i - 1].class == Class::Marker;

        // A time with no day attached means today. §3.1.5's `point` production
        // requires an anchor and a bare clock is not one, so "meeting at 14:00"
        // — about the most ordinary capture there is — had no production at all
        // and read as nothing. Found by pointing the recognizer at real
        // captures, which is what task 11.3 exists for.
        //
        // Today even when the time has already passed. A note saying "meeting
        // at 14:00" written at 15:00 is far more likely to be recording what
        // happened than scheduling tomorrow, and §3.1.3 prefers the reading
        // that invents less: a past entry is a record, a future one is a
        // commitment nobody made.
        if let Class::Clock { hour, minute, .. } = t.class {
            let today = today(now, zone).1;
            let ms = instant(today, hour * 60 + minute, zone)?;
            return Some((
                Shape::Point(When::Moment(ms)),
                if marked { band::MARKED } else { band::ADJACENT },
            ));
        }

        // `consumed` is how many tokens the anchor itself used, so the search
        // for a time of day starts after it. Without that the count in "in 3
        // weeks" is read as 03:00 — it follows a marker, which is exactly the
        // condition that makes a bare number a time.
        let (anchor, mut consumed, kind) = match t.class {
            // A weekday with a date beside it is that date, if they agree. The
            // date is the more precise of the two, and reading the weekday
            // alone dropped it — and the time after it — on the floor.
            Class::Weekday => match paired(tokens, i, i, now, zone) {
                Paired::Agrees(d, used) => (d, used, Anchor::CompleteDate),
                Paired::Disagrees(u) => {
                    *unclear_day = Some(u);
                    return None;
                }
                Paired::Alone => (next_weekday(now, zone, t.text)?, 1, Anchor::Day),
            },
            Class::DayWord => (day_word(now, zone, t.text)?, 1, Anchor::Day),
            // A date written in full, as one token, `sep 20`, or `20 sep`. A
            // full date is exactly as unambiguous as `sep 20` and read at the
            // same band; day-first is how most of the world writes one.
            Class::IsoDate { .. } | Class::Month | Class::Number(_) => {
                let (d, used) = complete_date(tokens, i, now, zone)?;
                (d, used, Anchor::CompleteDate)
            }
            Class::Quarter(q) => {
                // The coming one, by the same rule a bare "aug 14" follows.
                // "revisit in Q2" said in August means next April, not the
                // April that has already been — a forward marker resolving into
                // the past is a review prompt that can never come due.
                let month = (q as u8 - 1) * 3 + 1;
                let year = year_for(now, zone, month, 1);
                (FloatingDate::new(year, month, 1)?, 1, Anchor::Offset)
            }
            Class::Marker if t.text.eq_ignore_ascii_case("in") => {
                let Class::Number(n) = tokens.get(i + 1)?.class else {
                    continue;
                };
                let unit = tokens.get(i + 2)?;
                if unit.class != Class::Unit {
                    continue;
                }

                // "in 10 minutes" is a moment, not a day. Offsets shorter than
                // a day cannot be expressed as a floating date at all, and
                // rounding one to today would turn a ten-minute reminder into
                // an all-day entry. Another miss found on a real capture.
                if let Some(minutes) = sub_day_minutes(n, unit.text) {
                    return Some((
                        Shape::Point(When::Moment(now + minutes * 60_000)),
                        band::ADJACENT,
                    ));
                }

                (add_units(now, zone, n, unit.text)?, 3, Anchor::Offset)
            }
            Class::Determiner => {
                let next = tokens.get(i + 1)?;
                match next.class {
                    Class::Weekday => match paired(tokens, i, i + 1, now, zone) {
                        // "next thursday oct 22": the date says which.
                        Paired::Agrees(d, used) => (d, used, Anchor::CompleteDate),
                        Paired::Disagrees(u) => {
                            *unclear_day = Some(u);
                            return None;
                        }
                        Paired::Alone => match relative(now, zone, t.text, next.text)? {
                            Relative::Day(d) => (d, 2, Anchor::Day),
                            Relative::Either(a, b) => {
                                *unclear_day = Some(UnclearDay {
                                    choices: [a, b],
                                    start: t.start,
                                    end: next.end,
                                });
                                return None;
                            }
                        },
                    },
                    Class::Unit => (add_units(now, zone, 1, next.text)?, 2, Anchor::Offset),
                    _ => continue,
                }
            }
            _ => continue,
        };

        // "oct 22 thursday": the weekday after a date, which agrees or asks.
        if t.class != Class::Weekday
            && t.class != Class::Determiner
            && kind == Anchor::CompleteDate
            && let Some(w) = weekday_after(tokens, i + consumed)
        {
            let named = weekday_bit(tokens[w].text)?.trailing_zeros() as u8;
            if anchor.weekday()? == named {
                consumed = w + 1 - i;
            } else {
                *unclear_day = Some(disagreement(named, anchor, t.start, tokens[w].end)?);
                return None;
            }
        }

        // "tomorrow at sunset": a sun time on the day. A moment rather than a
        // day, so a task verb in front does not make it a due date.
        if let Some((sun, _)) = sun_at(tokens, i + consumed) {
            return Some(sun_point(anchor, sun));
        }

        // A clock immediately after, or after a marker.
        let after = tokens.get(i + consumed);
        let (minutes, vague) = match after.map(|t| t.class) {
            Some(Class::Clock { hour, minute, .. }) => (Some(hour * 60 + minute), false),
            Some(Class::TodWord) => (tod_minutes(after?.text), true),
            _ => match clock_after(tokens, i + consumed, unclear) {
                Some(m) => (Some(m), false),
                None => (None, false),
            },
        };

        // The bands of §3.1.5, in the order the document states them. A vague
        // time of day beats everything downward: "tomorrow morning" is a guess
        // dressed as a time whatever else is around it.
        let confidence = if vague {
            band::VAGUE
        } else {
            match (kind, marked, minutes.is_some()) {
                // "on aug 14", "by friday 9am" — a complete date behind a marker.
                (Anchor::CompleteDate, true, _) => band::MARKED,
                (Anchor::Day, true, true) => band::MARKED,
                // "thursday 2pm", "aug 14", "in 3 weeks" — two items adjacent.
                (Anchor::CompleteDate, false, _) => band::ADJACENT,
                (Anchor::Offset, _, _) => band::ADJACENT,
                (Anchor::Day, _, true) => band::ADJACENT,
                // "dentist thursday" — a bare day, and under the floor.
                (Anchor::Day, _, false) => band::BARE_DAY,
            }
        };

        let when = match minutes {
            Some(m) => When::Moment(instant(anchor, m, zone)?),
            None => When::Date(anchor),
        };

        // A day behind a task verb is a due date rather than an all-day event.
        // The one place the two tiers compose, decided here because this is
        // where the anchor kind is known — and the kind is what settles it.
        //
        // Three conditions, and every one of them was forced by a case that got
        // it wrong:
        //
        // - **A day, not a moment.** §3.6's table is about time, not wording: an
        //   event occupies time and has no state, a task has state and occupies
        //   none. "Call sarah at 3pm" is a call you are on at three.
        // - **A calendar day, not an offset.** "Check back in 3 weeks" is a
        //   forward nudge, not something you are overdue on come the 23rd — and
        //   §3.1's own worked example reads "revisit in Q2" as a note with a
        //   review prompt rather than a task. Caught by the existing band test,
        //   which is the only reason the distinction is here.
        // - **A verb at the head.** "Dentist thursday" is an appointment nobody
        //   phrased as a task, and it keeps its own band.
        if head_is_verb
            && matches!(kind, Anchor::Day | Anchor::CompleteDate)
            && let When::Date(d) = when
        {
            // Deliberately the composed band even where the date alone scored
            // higher. Confidence is in the whole reading, and the weakest claim
            // in it is that this is a task at all — a certain date does not make
            // that certain.
            return Some((Shape::Task { due: Some(d) }, band::TASK_DUE));
        }

        return Some((Shape::Point(when), confidence));
    }
    None
}

/// A complete date starting at `i`: `2026-10-22`, `oct 22`, `22 oct`, and how
/// many tokens it took.
fn complete_date(
    tokens: &[Token<'_>],
    i: usize,
    now: i64,
    zone: Zone,
) -> Option<(FloatingDate, usize)> {
    let t = tokens.get(i)?;
    match t.class {
        Class::IsoDate { year, month, day } => Some((FloatingDate::new(year, month, day)?, 1)),
        Class::Month => {
            let day = day_number(tokens.get(i + 1)?)?;
            let month = month_number(t.text)?;
            Some((
                FloatingDate::new(year_for(now, zone, month, day), month, day)?,
                2,
            ))
        }
        // A month name has to follow, which is what keeps `in 3 weeks` out: no
        // unit is a month.
        Class::Number(n) => {
            let month = month_number(tokens.get(i + 1)?.text)?;
            let day = u8::try_from(n).ok()?;
            Some((
                FloatingDate::new(year_for(now, zone, month, day), month, day)?,
                2,
            ))
        }
        _ => None,
    }
}

/// What a weekday at `w` makes of a date directly after it.
enum Paired {
    /// The date is that weekday: the date, and the tokens used from `from`.
    Agrees(FloatingDate, usize),
    /// It is not.
    Disagrees(UnclearDay),
    /// No date follows.
    Alone,
}

/// A weekday at `w`, the match having begun at `from`.
fn paired(tokens: &[Token<'_>], from: usize, w: usize, now: i64, zone: Zone) -> Paired {
    let mut j = w + 1;
    if tokens.get(j).is_some_and(|t| t.class == Class::Break) {
        j += 1;
    }
    let Some((date, used)) = complete_date(tokens, j, now, zone) else {
        return Paired::Alone;
    };
    let Some(named) = weekday_bit(tokens[w].text).map(|b| b.trailing_zeros() as u8) else {
        return Paired::Alone;
    };
    if date.weekday() == Some(named) {
        return Paired::Agrees(date, j + used - from);
    }
    match disagreement(named, date, tokens[from].start, tokens[j + used - 1].end) {
        Some(u) => Paired::Disagrees(u),
        None => Paired::Alone,
    }
}

/// A weekday directly after position `j`, or after one comma.
fn weekday_after(tokens: &[Token<'_>], mut j: usize) -> Option<usize> {
    if tokens.get(j)?.class == Class::Break {
        j += 1;
    }
    (tokens.get(j)?.class == Class::Weekday).then_some(j)
}

/// The question for a weekday and a date that are different days: the named
/// weekday nearest the date, or the date. Earlier first.
fn disagreement(named: u8, date: FloatingDate, start: usize, end: usize) -> Option<UnclearDay> {
    let delta = (named as i64 - date.weekday()? as i64).rem_euclid(7);
    let delta = if delta > 3 { delta - 7 } else { delta };
    let other = date.plus_days(delta)?;
    let choices = if other < date {
        [other, date]
    } else {
        [date, other]
    };
    Some(UnclearDay {
        choices,
        start,
        end,
    })
}

/// What a determiner makes of a weekday.
enum Relative {
    Day(FloatingDate),
    Either(FloatingDate, FloatingDate),
}

/// "next thursday", "last thursday", "this thursday".
///
/// "next" is asked only while this week's is still ahead (weeks from Monday, as
/// the calendar draws them): on a Tuesday, "next thursday" is this week's to
/// some people and next week's to others. Once this week's has passed both
/// readings are the coming one, and on the day itself both are a week away.
/// "last" is the most recent before today. Every other determiner keeps the
/// bare weekday's on-or-after, so "this thursday" said on a Thursday is today.
fn relative(now: i64, zone: Zone, det: &str, word: &str) -> Option<Relative> {
    let named = weekday_bit(word)?.trailing_zeros() as i64;
    let today = today(now, zone).1;
    let current = today.weekday()? as i64;
    match det.to_ascii_lowercase().as_str() {
        "next" => {
            let days = match (named - current).rem_euclid(7) {
                0 => 7,
                n => n,
            };
            let coming = today.plus_days(days)?;
            if current + days <= 6 {
                Some(Relative::Either(coming, coming.plus_days(7)?))
            } else {
                Some(Relative::Day(coming))
            }
        }
        "last" => {
            let days = match (current - named).rem_euclid(7) {
                0 => 7,
                n => n,
            };
            Some(Relative::Day(today.plus_days(-days)?))
        }
        _ => Some(Relative::Day(next_weekday(now, zone, word)?)),
    }
}

// ---------------------------------------------------------------- guards ----

/// Whether a token is allowed to begin a temporal match (§3.1.5).
///
/// These are not part of the grammar because they are about what surrounds a
/// match rather than its structure, and they are the half that keeps this off
/// ordinary prose.
fn guarded(tokens: &[Token<'_>], t: &Token<'_>) -> bool {
    let Some(i) = tokens.iter().position(|x| x.start == t.start) else {
        return false;
    };

    // An article before a month, weekday or clock means the word is naming
    // something rather than dating it: "the march timeline", "the 7am problem".
    if i > 0 && tokens[i - 1].class == Class::Blocker {
        return false;
    }

    // A following noun kills a bare weekday or month — "friday deploy" is a
    // name. A vocabulary word after it is fine, because that is the grammar.
    if matches!(t.class, Class::Weekday | Class::Month)
        && let Some(next) = tokens.get(i + 1)
        && next.class == Class::Word
    {
        return false;
    }

    // A match starts at the head of the capture, at a phrase boundary, or after
    // a marker — never mid-clause.
    if i == 0 {
        return true;
    }
    matches!(
        tokens[i - 1].class,
        Class::Marker | Class::Break | Class::Determiner | Class::RecHead
    ) || matches!(
        t.class,
        // A full date needs no permission from what precedes it. It is the most
        // distinctive token there is — ten characters, a four-digit year, a real
        // calendar day — and there is no prose in which `2026-09-20` is not a
        // date. The other classes here are self-evident; this one more so.
        Class::IsoDate { .. }
            | Class::Weekday
            | Class::Month
            | Class::DayWord
            | Class::Quarter(_)
            | Class::RecHead
            | Class::FreqWord
            | Class::Determiner
            // A marker is itself the permission. §3.1.5 allows a match to start
            // "after a marker", and a match that *begins* with one — "check back
            // in 3 weeks" — is the same rule seen from the other side. Without
            // this, `in 3 weeks` could only ever be recognised at the very start
            // of a capture, which is not where people put it.
            | Class::Marker
    )
}

// ------------------------------------------------------------ resolution ----

fn today(now: i64, zone: Zone) -> (i32, FloatingDate) {
    let d = capture_time::civil_date(now, zone);
    (d.year, d)
}

fn next_weekday(now: i64, zone: Zone, word: &str) -> Option<FloatingDate> {
    let bit = weekday_bit(word)?;
    let wanted = bit.trailing_zeros();
    capture_time::next_weekday_on_or_after(now, zone, wanted as u8)
}

fn day_word(now: i64, zone: Zone, word: &str) -> Option<FloatingDate> {
    let offset = match word.to_ascii_lowercase().as_str() {
        "today" | "tonight" => 0,
        "tomorrow" => 1,
        "yesterday" => -1,
        _ => return None,
    };
    capture_time::add_days(now, zone, offset)
}

/// An offset shorter than a day, in minutes.
///
/// `None` for anything a day or longer, which is the caller's signal to resolve
/// a date instead.
fn sub_day_minutes(n: u32, unit: &str) -> Option<i64> {
    match unit.to_ascii_lowercase().as_str() {
        "minute" | "minutes" | "min" | "mins" => Some(n as i64),
        "hour" | "hours" | "hr" | "hrs" => Some(n as i64 * 60),
        _ => None,
    }
}

fn add_units(now: i64, zone: Zone, n: u32, unit: &str) -> Option<FloatingDate> {
    let days = match unit.to_ascii_lowercase().as_str() {
        "day" | "days" => n as i64,
        "week" | "weeks" | "wk" | "wks" => n as i64 * 7,
        "month" | "months" | "mo" | "mos" => n as i64 * 30,
        "year" | "years" | "yr" | "yrs" => n as i64 * 365,
        "quarter" | "quarters" => n as i64 * 91,
        _ => return None,
    };
    capture_time::add_days(now, zone, days)
}

/// Which year a bare `aug 14` means.
///
/// The coming one. A date already past this year is almost always next year's —
/// somebody writing "aug 14" in September means the August after, not the one
/// that has been and gone.
fn year_for(now: i64, zone: Zone, month: u8, day: u8) -> i32 {
    let (year, today) = today(now, zone);
    let candidate = (month, day);
    let current = (today.month, today.day);
    if candidate < current { year + 1 } else { year }
}

fn instant(date: FloatingDate, minutes: u32, zone: Zone) -> Option<i64> {
    capture_time::at_local(date, minutes / 60, minutes % 60, zone)
}
