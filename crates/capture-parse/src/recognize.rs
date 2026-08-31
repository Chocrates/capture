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
}

/// Read a capture.
///
/// `now` is the moment to resolve relative expressions against, and `zone` the
/// zone the capture was written in. Both are arguments rather than ambient, so
/// the same text read twice gives the same answer.
pub fn recognize(text: &str, now: i64, zone: Zone) -> Recognition {
    let tokens = tokenize(text);
    let markers = markers(&tokens);

    // A sigil is exact and beats inference. Where one names a date, tier 2 is
    // not consulted for one at all — not even to record a competing suggestion
    // (§3.1.1).
    if let Some(d) = markers.date.as_deref().and_then(FloatingDate::parse) {
        return Recognition {
            shape: Shape::Point(When::Date(d)),
            confidence: 100,
            markers,
            grammar: crate::GRAMMAR_VERSION,
        };
    }

    // A task verb at the head is a hint about what the sentence is *for*.
    // Verbs are an open class and any list of them is wrong at the edges, so it
    // is never the whole reading — but it does decide between two readings of
    // the same day, and that is what the composition below is.
    let head_is_verb = tokens
        .iter()
        .find(|t| t.class != Class::Break)
        .is_some_and(|t| t.class == Class::TaskVerb);

    if let Some((shape, confidence)) = temporal(&tokens, now, zone, head_is_verb) {
        return Recognition {
            shape,
            confidence,
            markers,
            grammar: crate::GRAMMAR_VERSION,
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
    }
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
) -> Option<(Shape, u32)> {
    // Recurrence first: `every weekday at 7` also contains a weekday, and the
    // longer reading is the one meant.
    if let Some(r) = recurrence(tokens) {
        return Some(r);
    }
    if let Some(r) = range(tokens, now, zone) {
        return Some(r);
    }
    // `head_is_verb` reaches only `point`, because it can only change one
    // reading: a day into a due date. A range is a span of days and a
    // recurrence is a rule, and neither becomes a task by being introduced by a
    // verb — "review the docs every friday" is still a recurring obligation.
    point(tokens, now, zone, head_is_verb)
}

/// `every weekday at 7`, `each tuesday`, `daily`.
fn recurrence(tokens: &[Token<'_>]) -> Option<(Shape, u32)> {
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
                rule.by_day = if word.starts_with("weekend") {
                    0b0110_0000 // Sat, Sun
                } else {
                    0b0001_1111 // Mon..Fri
                };
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

    Some((
        Shape::Recurring {
            rule,
            at_minute: clock_after(tokens, i),
        },
        band::RECURRENCE,
    ))
}

/// A time of day appearing after position `i`, with or without a marker.
fn clock_after(tokens: &[Token<'_>], mut i: usize) -> Option<u32> {
    while i < tokens.len() {
        match tokens[i].class {
            Class::Marker => i += 1,
            Class::Clock { hour, minute, .. } => return Some(hour * 60 + minute),
            Class::TodWord => return tod_minutes(tokens[i].text),
            // A bare number is a time here only because a marker preceded it:
            // "every weekday at 7". Without the marker it stays a number.
            Class::Number(n) if n <= 23 && i > 0 && tokens[i - 1].class == Class::Marker => {
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
fn point(tokens: &[Token<'_>], now: i64, zone: Zone, head_is_verb: bool) -> Option<(Shape, u32)> {
    for (i, t) in tokens.iter().enumerate() {
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
        let (anchor, consumed, kind) = match t.class {
            Class::Weekday => (next_weekday(now, zone, t.text)?, 1, Anchor::Day),
            Class::DayWord => (day_word(now, zone, t.text)?, 1, Anchor::Day),
            Class::Month => {
                let day = day_number(tokens.get(i + 1)?)?;
                let month = month_number(t.text)?;
                (
                    FloatingDate::new(year_for(now, zone, month, day), month, day)?,
                    2,
                    Anchor::CompleteDate,
                )
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
                    Class::Weekday => (next_weekday(now, zone, next.text)?, 2, Anchor::Day),
                    Class::Unit => (add_units(now, zone, 1, next.text)?, 2, Anchor::Offset),
                    _ => continue,
                }
            }
            _ => continue,
        };

        // A clock immediately after, or after a marker.
        let after = tokens.get(i + consumed);
        let (minutes, vague) = match after.map(|t| t.class) {
            Some(Class::Clock { hour, minute, .. }) => (Some(hour * 60 + minute), false),
            Some(Class::TodWord) => (tod_minutes(after?.text), true),
            _ => match clock_after(tokens, i + consumed) {
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
        Class::Weekday
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
