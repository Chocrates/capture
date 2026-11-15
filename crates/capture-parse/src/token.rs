//! Splitting a capture into tokens the grammar can read.
//!
//! # Classified against the closed vocabulary, and nothing else
//!
//! Every token is looked up in the tables of `DESIGN.md` §3.1.5 and gets the
//! class it is found in, or [`Class::Word`] if it is not found at all. There is
//! no fuzzy matching, no stemming and no edit distance: a word outside the list
//! is not a near miss to be tuned, it is tier 3's problem, and tier 3 declining
//! leaves an ordinary note (§3.1.2).
//!
//! # Spans, because the guards are about position
//!
//! Each token carries where it came from. The grammar could work without that,
//! but the guards cannot — "an article before a month" and "a match starts at
//! the head, at a comma, or after a marker" are both statements about
//! neighbours, and both matter more than the grammar itself (§3.1.5).
//!
//! # What is deliberately not done here
//!
//! No normalisation of the text, ever. The capture is stored verbatim and this
//! reads a borrowed copy of it: lowercasing happens per comparison rather than
//! by rewriting, so nothing here can produce a version of the note that differs
//! from what was typed.

use alloc::vec::Vec;

use crate::{
    BLOCKERS, DAY_SETS, DAY_WORDS, DETERMINERS, FREQ_WORDS, INTERVALS, MARKERS, MERIDIEM, MONTHS,
    RANGE_OPS, REC_HEADS, SIGILS, SUN_WORDS, TASK_VERBS, TIME_WORDS, UNITS, WEEKDAYS, Word,
};

/// What a token turned out to be.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Weekday,
    Month,
    DayWord,
    TodWord,
    Unit,
    FreqWord,
    RecHead,
    DaySet,
    Determiner,
    Marker,
    RangeOp,
    Meridiem,
    Interval,
    TaskVerb,
    /// `sunrise`, `sunset`: a time of day only behind `at`, `before`, `after`.
    SunWord,
    /// `the`, `a`, `an` — the guard that kills a match (§3.1.5).
    Blocker,
    /// A bare number. Never a time on its own: it needs a colon, a meridiem or
    /// a marker, or "budget 40k" becomes 40:00.
    Number(u32),
    /// `Q1`–`Q4`.
    Quarter(u32),
    /// A full date written as one token: `2026-09-20`.
    ///
    /// Decided here, where every other shape is decided. A date is exactly the
    /// question a tokenizer answers — "what is this run of characters" — and
    /// putting it anywhere else means two places that know what one looks like.
    ///
    /// Validity is checked when the class is assigned, so `2026-13-45` stays an
    /// ordinary word rather than becoming a date that fails later. The whole
    /// token must be the date, which is what keeps `v2026-09-20` a word.
    IsoDate {
        year: i32,
        month: u8,
        day: u8,
    },
    /// `1st`, `2nd`, `14th`.
    Ordinal(u32),
    /// A clock written as one token: `2pm`, `14:00`, `9:30am`.
    Clock {
        hour: u32,
        minute: u32,
        /// Whether a meridiem was written. A bare `14:00` is unambiguous; a
        /// bare `2` is not a clock at all.
        explicit: bool,
    },
    /// An explicit marker and what followed it, e.g. `#tag`.
    Sigil(char),
    /// A word the vocabulary does not contain.
    Word,
    /// A comma or full stop — a phrase boundary the guards care about.
    Break,
}

/// One token, and where it was.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Token<'a> {
    /// The token exactly as written.
    pub text: &'a str,
    /// Byte offset of its first character in the capture.
    pub start: usize,
    /// Byte offset just past its last character.
    pub end: usize,
    /// What it was recognised as.
    pub class: Class,
}

/// Punctuation that ends a sentence rather than a value.
///
/// Trimmed from the *end* of a sigil's value and nowhere else. A sigil owns
/// everything up to whitespace (see `tokenize`, which has to, so that
/// `>2026-12-25` survives as one date) — and the cost of that was silent: a
/// capture reading "finished it #reading." filed itself under the tag
/// `reading.`, which is a different tag from `reading` and looks identical in
/// every list it appears in. The one organising axis somebody writes by hand
/// split itself on whether a sentence had a full stop.
///
/// Deliberately not `-`, `_` or `/`: those appear inside real values
/// (`^acme-migration`, `>2026-12-25`) and trimming them would corrupt the
/// thing this is trying to keep whole.
const TRAILING: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}', '"', '\''];

impl<'a> Token<'a> {
    /// The part after the sigil character, for a sigil token.
    ///
    /// Trailing sentence punctuation is trimmed; the token's own span is not,
    /// because the grammar's guards are stated over where a token *is* and
    /// narrowing that here would quietly change what matches.
    pub fn sigil_value(&self) -> Option<&'a str> {
        match self.class {
            Class::Sigil(_) => {
                let v = self.text[1..].trim_end_matches(TRAILING);
                // A sigil with nothing left is not a mention of anything. It
                // used to be reported as one with an empty name, which put a
                // nameless entry on every page that lists what a note names.
                (!v.is_empty()).then_some(v)
            }
            _ => None,
        }
    }
}

fn found(table: &[Word], s: &str) -> bool {
    table.iter().any(|w| w.word.eq_ignore_ascii_case(s))
}

/// Split a capture into classified tokens.
pub fn tokenize(text: &str) -> Vec<Token<'_>> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;

    while i < bytes.len() {
        let c = bytes[i];

        // Whitespace separates and carries no meaning.
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // A phrase boundary. The guards use these: a match may begin after one,
        // and a temporal expression does not run across one.
        if c == b',' || c == b';' || c == b'.' && !is_meridiem_dot(bytes, i) {
            out.push(Token {
                text: &text[i..i + 1],
                start: i,
                end: i + 1,
                class: Class::Break,
            });
            i += 1;
            continue;
        }

        // A date written in full, before the range operator gets to it.
        //
        // This ordering is the whole fix. A hyphen between digits is a range —
        // `aug 1-14` — so `2026-09-20` was split into three numbers and two
        // operators and never reached the classifier as one thing. The most
        // unambiguous way to write a date was the one way the archive could not
        // see it at all.
        //
        // Safe to try first because the shape is exact: ten characters, a
        // four-digit year, and a real calendar day. `1-14` cannot match it, and
        // neither can `2026-13-45`.
        if c.is_ascii_digit()
            && let Some(end) = iso_date_end(text, i)
        {
            let whole = &text[i..end];
            if let Some(d) = capture_time::FloatingDate::parse(whole) {
                out.push(Token {
                    text: whole,
                    start: i,
                    end,
                    class: Class::IsoDate {
                        year: d.year,
                        month: d.month,
                        day: d.day,
                    },
                });
                i = end;
                continue;
            }
        }

        // A range written tight: `1-14`, `aug 1..14`.
        if (c == b'-' || c == b'.') && looks_like_range(bytes, i) {
            let len = if c == b'.' { 2 } else { 1 };
            out.push(Token {
                text: &text[i..i + len],
                start: i,
                end: i + len,
                class: Class::RangeOp,
            });
            i += len;
            continue;
        }

        let start = i;
        let sigil = SIGILS.iter().find(|s| s.sigil as u8 == c);

        // A sigil owns the run that follows it, so `#tag` is one token and the
        // grammar cannot mistake `tag` for an ordinary word.
        if sigil.is_some() {
            i += 1;
        }

        // A sigil owns everything up to whitespace, punctuation included. It
        // has to: `>2026-12-25` is one explicit date, and letting the range
        // rule split it on its hyphens turned the most exact thing a person can
        // write into three tokens the grammar then ignored.
        let owns_all = sigil.is_some();
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && (owns_all || !is_terminator(bytes, i))
        {
            i += 1;
        }

        if i == start {
            // A lone punctuation character that is neither a sigil nor a break.
            //
            // By characters rather than by bytes. Only an ASCII terminator can
            // stop the loop above without advancing, so a single byte is right
            // today — and would stop being right the moment anything non-ASCII
            // joined that list, with the failure being a panic in the middle of
            // a commit rather than a wrong token.
            i += text[i..].chars().next().map_or(1, char::len_utf8);
        }

        let text_of = &text[start..i];
        let class = match sigil {
            // A sigil with nothing after it is just punctuation somebody typed.
            Some(s) if text_of.len() > 1 => Class::Sigil(s.sigil),
            _ => classify(text_of),
        };

        out.push(Token {
            text: text_of,
            start,
            end: i,
            class,
        });
    }

    out
}

/// Whether the byte at `i` ends the current token.
fn is_terminator(bytes: &[u8], i: usize) -> bool {
    match bytes[i] {
        b',' | b';' => true,
        b'.' => !is_meridiem_dot(bytes, i),
        b'-' => looks_like_range(bytes, i),
        _ => false,
    }
}

/// A dot inside `a.m.` / `p.m.`, which is part of the word rather than a break.
fn is_meridiem_dot(bytes: &[u8], i: usize) -> bool {
    // Look at the two characters before and after: `a.m.` reads as a dot with a
    // single letter on at least one side.
    let before = i.checked_sub(1).map(|j| bytes[j]).unwrap_or(b' ');
    let after = bytes.get(i + 1).copied().unwrap_or(b' ');
    matches!(before, b'a' | b'p' | b'm' | b'A' | b'P' | b'M')
        && matches!(after, b'm' | b'M' | b' ' | b'\t' | b'\n')
}

/// Where a full date ends, if one starts here.
///
/// Exactly ten characters and a boundary after them, so `v2026-09-20` is not a
/// date with a letter in front and `2026-09-201` is not one with a digit after.
/// Whether those ten characters are a *real* day is left to the parser, which
/// already knows that the 31st of February is not one.
fn iso_date_end(text: &str, at: usize) -> Option<usize> {
    let end = at + 10;
    if end > text.len() || !text.is_char_boundary(end) {
        return None;
    }
    let b = text.as_bytes();
    if at > 0 && !b[at - 1].is_ascii_whitespace() && !matches!(b[at - 1], b'>' | b'(' | b'[') {
        return None;
    }
    if end < b.len() && (b[end].is_ascii_alphanumeric() || b[end] == b'-') {
        return None;
    }
    Some(end)
}

/// Whether this is a range operator rather than a hyphen inside a word.
fn looks_like_range(bytes: &[u8], i: usize) -> bool {
    if bytes[i] == b'.' {
        return bytes.get(i + 1) == Some(&b'.');
    }
    let before = i.checked_sub(1).map(|j| bytes[j]).unwrap_or(b' ');
    let after = bytes.get(i + 1).copied().unwrap_or(b' ');
    before.is_ascii_digit() && after.is_ascii_digit()
}

/// Which class a token belongs to, if any.
fn classify(s: &str) -> Class {
    if let Some(c) = clock(s) {
        return c;
    }
    if let Some(q) = quarter(s) {
        return Class::Quarter(q);
    }
    if let Some(d) = capture_time::FloatingDate::parse(s) {
        return Class::IsoDate {
            year: d.year,
            month: d.month,
            day: d.day,
        };
    }
    if let Some(o) = ordinal(s) {
        return Class::Ordinal(o);
    }
    if let Ok(n) = s.parse::<u32>() {
        return Class::Number(n);
    }

    // Order matters where a word is in two tables. `until` is both a marker and
    // a range operator (§3.1.5 records it as the single such word); it is
    // classified as a marker here and the grammar decides positionally.
    if found(MARKERS, s) {
        return Class::Marker;
    }
    for (table, class) in [
        (WEEKDAYS, Class::Weekday),
        (MONTHS, Class::Month),
        (DAY_WORDS, Class::DayWord),
        (TIME_WORDS, Class::TodWord),
        (FREQ_WORDS, Class::FreqWord),
        (REC_HEADS, Class::RecHead),
        (DAY_SETS, Class::DaySet),
        (DETERMINERS, Class::Determiner),
        (RANGE_OPS, Class::RangeOp),
        (MERIDIEM, Class::Meridiem),
        (INTERVALS, Class::Interval),
        (BLOCKERS, Class::Blocker),
        (UNITS, Class::Unit),
        (TASK_VERBS, Class::TaskVerb),
        (SUN_WORDS, Class::SunWord),
    ] {
        if found(table, s) {
            return class;
        }
    }
    Class::Word
}

/// `Q1`–`Q4`.
fn quarter(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.len() == 2 && (b[0] == b'q' || b[0] == b'Q') && (b'1'..=b'4').contains(&b[1]) {
        return Some((b[1] - b'0') as u32);
    }
    None
}

/// `1st`, `2nd`, `3rd`, `14th`.
fn ordinal(s: &str) -> Option<u32> {
    // `split_at_checked`, because the index is in bytes and the text is not.
    //
    // `split_at` panics when the cut lands inside a character, and the last two
    // bytes of a word are inside one whenever it ends in anything wider than
    // ASCII: an em dash, an ellipsis, an emoji, an arrow. A capture containing
    // one killed the process during commit — after the event was durable, so
    // nothing was lost, but the writer died and the projection was left behind
    // the log. Found by writing an ordinary sentence with a dash in it.
    let (digits, suffix) = s.split_at_checked(s.len().checked_sub(2)?)?;
    if !matches!(
        suffix.to_ascii_lowercase().as_str(),
        "st" | "nd" | "rd" | "th"
    ) {
        return None;
    }
    let n: u32 = digits.parse().ok()?;
    (1..=31).contains(&n).then_some(n)
}

/// A clock written as one token.
///
/// `2pm`, `14:00`, `9:30am`, `noon` is elsewhere. A bare number is deliberately
/// *not* a clock: that is the guard which keeps "budget 40k" from becoming a
/// time, and it is enforced by this returning `None` for anything without a
/// colon or a meridiem.
fn clock(s: &str) -> Option<Class> {
    let lower = s.to_ascii_lowercase();
    let (body, meridiem) = if let Some(rest) = lower.strip_suffix("am") {
        (rest, Some(false))
    } else if let Some(rest) = lower.strip_suffix("pm") {
        (rest, Some(true))
    } else if let Some(rest) = lower.strip_suffix("a.m.") {
        (rest, Some(false))
    } else if let Some(rest) = lower.strip_suffix("p.m.") {
        (rest, Some(true))
    } else {
        (lower.as_str(), None)
    };

    let body = body.trim_end_matches('.');
    let (h, m) = match body.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
        None => {
            // No colon: only a meridiem makes this a time at all.
            meridiem?;
            (body.parse::<u32>().ok()?, 0)
        }
    };

    if m > 59 {
        return None;
    }
    let hour = match meridiem {
        Some(true) if h < 12 => h + 12,
        Some(false) if h == 12 => 0,
        _ => h,
    };
    if hour > 23 {
        return None;
    }

    Some(Class::Clock {
        hour,
        minute: m,
        explicit: meridiem.is_some() || body.contains(':'),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn classes(text: &str) -> Vec<Class> {
        tokenize(text).into_iter().map(|t| t.class).collect()
    }

    #[test]
    fn a_word_outside_the_vocabulary_is_simply_a_word() {
        // No stemming, no edit distance. An unlisted word is tier 3's problem,
        // not a near miss to be tuned.
        assert_eq!(classes("pangolin"), alloc::vec![Class::Word]);
        assert_eq!(classes("thursdayish"), alloc::vec![Class::Word]);
    }

    #[test]
    fn the_vocabulary_classes_are_recognised() {
        assert_eq!(classes("thursday"), alloc::vec![Class::Weekday]);
        assert_eq!(classes("THURSDAY"), alloc::vec![Class::Weekday]);
        assert_eq!(classes("aug"), alloc::vec![Class::Month]);
        assert_eq!(classes("tomorrow"), alloc::vec![Class::DayWord]);
        assert_eq!(classes("noon"), alloc::vec![Class::TodWord]);
        assert_eq!(classes("weeks"), alloc::vec![Class::Unit]);
        assert_eq!(classes("daily"), alloc::vec![Class::FreqWord]);
        assert_eq!(classes("every"), alloc::vec![Class::RecHead]);
        assert_eq!(classes("weekdays"), alloc::vec![Class::DaySet]);
        assert_eq!(classes("next"), alloc::vec![Class::Determiner]);
        assert_eq!(classes("the"), alloc::vec![Class::Blocker]);
        assert_eq!(classes("send"), alloc::vec![Class::TaskVerb]);
    }

    #[test]
    fn until_is_a_marker_and_the_grammar_decides_the_rest() {
        // The one word in two classes. Classified once, positionally resolved.
        assert_eq!(classes("until"), alloc::vec![Class::Marker]);
    }

    #[test]
    fn a_clock_needs_a_colon_or_a_meridiem() {
        // The guard that keeps "budget 40k" from becoming 40:00, enforced here
        // rather than left to the grammar.
        assert_eq!(
            classes("2pm"),
            alloc::vec![Class::Clock {
                hour: 14,
                minute: 0,
                explicit: true
            }]
        );
        assert_eq!(
            classes("14:00"),
            alloc::vec![Class::Clock {
                hour: 14,
                minute: 0,
                explicit: true
            }]
        );
        assert_eq!(
            classes("9:30am"),
            alloc::vec![Class::Clock {
                hour: 9,
                minute: 30,
                explicit: true
            }]
        );
        assert_eq!(classes("40"), alloc::vec![Class::Number(40)]);
        assert_eq!(classes("7"), alloc::vec![Class::Number(7)]);
    }

    #[test]
    fn midnight_and_noon_convert_the_way_people_mean_them() {
        assert_eq!(
            classes("12am"),
            alloc::vec![Class::Clock {
                hour: 0,
                minute: 0,
                explicit: true
            }]
        );
        assert_eq!(
            classes("12pm"),
            alloc::vec![Class::Clock {
                hour: 12,
                minute: 0,
                explicit: true
            }]
        );
    }

    #[test]
    fn an_impossible_clock_is_not_a_clock() {
        assert_eq!(classes("25:00"), alloc::vec![Class::Word]);
        assert_eq!(classes("10:75"), alloc::vec![Class::Word]);
    }

    #[test]
    fn quarters_and_ordinals_carry_their_number() {
        assert_eq!(classes("Q2"), alloc::vec![Class::Quarter(2)]);
        assert_eq!(classes("q4"), alloc::vec![Class::Quarter(4)]);
        assert_eq!(classes("Q5"), alloc::vec![Class::Word]);
        assert_eq!(classes("14th"), alloc::vec![Class::Ordinal(14)]);
        assert_eq!(classes("1st"), alloc::vec![Class::Ordinal(1)]);
        assert_eq!(
            classes("40th"),
            alloc::vec![Class::Word],
            "no 40th of a month"
        );
    }

    #[test]
    fn a_sigil_owns_what_follows_it() {
        let t = tokenize("call mum #family @mum");
        assert_eq!(t[2].class, Class::Sigil('#'));
        assert_eq!(t[2].sigil_value(), Some("family"));
        assert_eq!(t[3].class, Class::Sigil('@'));
        assert_eq!(t[3].sigil_value(), Some("mum"));
    }

    #[test]
    fn a_bare_sigil_is_punctuation_somebody_typed() {
        // "#" alone is not a tag, and treating it as one would make an empty
        // tag out of a stray character.
        let t = tokenize("what # even");
        assert_ne!(t[1].class, Class::Sigil('#'));
    }

    #[test]
    fn a_tight_range_is_split_but_a_hyphenated_word_is_not() {
        let t = tokenize("aug 1-14");
        assert_eq!(
            t.iter().map(|t| t.class).collect::<Vec<_>>(),
            alloc::vec![
                Class::Month,
                Class::Number(1),
                Class::RangeOp,
                Class::Number(14)
            ]
        );
        // A hyphen between letters belongs to the word.
        assert_eq!(classes("well-known"), alloc::vec![Class::Word]);
    }

    #[test]
    fn a_comma_is_a_phrase_boundary() {
        // The guards need these: a match may begin after one, and a temporal
        // expression does not run across one.
        let t = tokenize("budget 40k, revisit in Q2");
        assert!(t.iter().any(|t| t.class == Class::Break));
    }

    #[test]
    fn spans_point_back_at_the_original_text() {
        // The guards are statements about neighbours, so position has to
        // survive tokenizing.
        let text = "dentist thursday 2pm";
        for t in tokenize(text) {
            assert_eq!(&text[t.start..t.end], t.text);
        }
    }

    #[test]
    fn nothing_is_normalised() {
        // The capture is stored verbatim and this reads a borrowed copy.
        // Lowercasing happens per comparison, never by rewriting.
        let text = "Call Mum THURSDAY";
        let t = tokenize(text);
        assert_eq!(t[0].text, "Call");
        assert_eq!(t[2].text, "THURSDAY");
        assert_eq!(t[2].class, Class::Weekday);
    }
}
