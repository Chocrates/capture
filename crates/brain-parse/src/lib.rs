//! The recognizer's vocabulary — the closed word list of `DESIGN.md` §3.1.5.
//!
//! # What is here, and what is not
//!
//! The tables. Not the parser: that is M2, and it will live in this crate
//! beside them. Splitting the two this way is deliberate — the vocabulary is a
//! design decision that other code needs *now* (the capture overlay shows it,
//! §3.1.1's "discovered by seeing them"), while the parser is a body of work
//! that depends on `brain-time`. Naming the words in a crate rather than in
//! prose means the overlay and the parser cannot drift from each other or from
//! the document, because there is only one list.
//!
//! # Why closed
//!
//! Every word tier 2 recognises is in this file. That is what makes the tier
//! exhaustively testable rather than representatively testable, and it is what
//! makes a miss legible: a word that is absent is not a near-miss to be tuned,
//! it is tier 3's problem, and tier 3 declining leaves an ordinary note. Adding
//! a word is an edit here with a test attached, never an inference at runtime.
//!
//! # Why `no_std`, and why no dependencies
//!
//! The same reason `brain-core` is. A capture must read the same on every
//! device that ever folds it, so the language it is read in cannot reach a
//! clock, a locale database, or the filesystem. Under `no_std` it cannot.
//!
//! # The grammar
//!
//! Specified in `DESIGN.md` §3.1.5 and reproduced here so it sits next to the
//! words it is written over:
//!
//! ```ebnf
//! capture     = { sigil | temporal | word } ;
//!
//! sigil       = "#" slug | "@" slug | ">" ( iso_date | day_ref )
//!             | "!" "p" digit | "~" number unit | "^" slug ;
//!
//! temporal    = recurrence | range | point ;
//!
//! recurrence  = rec_head [ interval | number ] rec_body [ [ "at" ] clock ]
//!             | freq_word [ [ "at" ] clock ] ;
//! rec_body    = weekday | day_set | unit ;
//!
//! range       = month day_num range_op day_num [ year ]
//!             | point range_op point ;
//!
//! point       = [ marker ] anchor [ [ "at" ] clock ] ;
//! anchor      = iso_date
//!             | month day_num [ year ]
//!             | day_num month [ year ]
//!             | weekday
//!             | day_word
//!             | quarter
//!             | "in" number unit
//!             | determiner ( weekday | unit | quarter ) ;
//!
//! clock       = hour [ ":" minute ] [ meridiem ]
//!             | tod_word ;
//! ```
//!
//! The guards that keep this from firing on ordinary prose — an article kills a
//! match, a bare number is never a time, a following noun kills a bare weekday
//! — are §3.1.5's, and they belong to the parser rather than to these tables.

#![no_std]

extern crate alloc;

mod recognize;
mod token;

pub use recognize::{Markers, Recognition, Shape, When, recognize};
pub use token::{Class, Token, tokenize};

/// The version of the vocabulary, the grammar and the bands, together.
///
/// Bumped whenever any of the three changes. Every promotion records the
/// version it was parsed under, and startup reparses anything older.
///
/// This exists because nothing in the system observes a build. A parser
/// improving is not an event, so without a version written into the data a note
/// captured in March could never benefit from a grammar fix shipped in June —
/// the improvement would apply only to whatever was captured after it.
///
/// Version 2 added two productions that real captures needed and the grammar
/// could not express: a bare clock meaning today ("meeting at 14:00"), and
/// offsets shorter than a day ("in 10 minutes"). Both were found by pointing
/// the recognizer at an actual store rather than at its own examples.
///
/// Version 3 made the two tiers compose. A task verb in front of a day was
/// read as the day alone and the verb was discarded, so "call the dentist
/// tomorrow" became an all-day event on Thursday and "finish the deck by
/// friday" became one on Friday — neither of which occupies the day it names
/// (§3.6). It is now a task with a floating due date, at a band of its own.
///
/// Version 4 changes no grammar at all, and is here because the version is the
/// only way to reach notes already promoted. The sweep read every note against
/// its own clock rather than against the capture, so a reparse *rescheduled*
/// what it read — "meeting at 14:00", captured yesterday, came back resolved to
/// today, and would have moved again on every later bump. Anything promoted
/// under 3 or earlier may be sitting on the day a sweep happened to run, so it
/// is read again now that a reparse resolves against the capture.
pub const GRAMMAR_VERSION: u32 = 4;

/// A word the recognizer knows, and what it is worth.
///
/// `minutes` carries the fixed numeric meaning where a word has one: the clock
/// a `tod_word` stands for, the length a `unit` measures. Words whose meaning
/// is positional rather than numeric leave it `None`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word {
    /// The word as it is written, lowercase.
    pub word: &'static str,
    /// What it means numerically, where that is a fixed thing.
    pub minutes: Option<u32>,
}

/// A word with no fixed numeric meaning.
const fn w(word: &'static str) -> Word {
    Word {
        word,
        minutes: None,
    }
}

/// A word that stands for a fixed number of minutes.
const fn n(word: &'static str, minutes: u32) -> Word {
    Word {
        word,
        minutes: Some(minutes),
    }
}

/// Days of the week, and the abbreviations people actually type.
pub const WEEKDAYS: &[Word] = &[
    w("monday"),
    w("mon"),
    w("tuesday"),
    w("tue"),
    w("tues"),
    w("wednesday"),
    w("wed"),
    w("thursday"),
    w("thu"),
    w("thur"),
    w("thurs"),
    w("friday"),
    w("fri"),
    w("saturday"),
    w("sat"),
    w("sunday"),
    w("sun"),
];

/// Months, long and short.
///
/// `may` is its own abbreviation, and `sept` is included alongside `sep`
/// because both are written by hand often enough to matter.
pub const MONTHS: &[Word] = &[
    w("january"),
    w("jan"),
    w("february"),
    w("feb"),
    w("march"),
    w("mar"),
    w("april"),
    w("apr"),
    w("may"),
    w("june"),
    w("jun"),
    w("july"),
    w("jul"),
    w("august"),
    w("aug"),
    w("september"),
    w("sep"),
    w("sept"),
    w("october"),
    w("oct"),
    w("november"),
    w("nov"),
    w("december"),
    w("dec"),
];

/// Days named relative to the day of capture.
///
/// `yesterday` is recognised even though nothing is ever scheduled into the
/// past: a capture that refers to it is dating its own content, and reading
/// that correctly is worth more than the handful of promotions it prevents.
pub const DAY_WORDS: &[Word] = &[w("today"), w("tonight"), w("tomorrow"), w("yesterday")];

/// Times of day, and the clock each one stands for.
///
/// Every one of these is a guess dressed as a time, which is why §3.1.5 scores
/// them at 0.60 — under the floor. They become suggestions, not promotions.
pub const TIME_WORDS: &[Word] = &[
    n("midnight", 0),
    n("morning", 9 * 60),
    n("noon", 12 * 60),
    n("afternoon", 14 * 60),
    n("evening", 18 * 60),
    n("night", 20 * 60),
];

/// Units of duration, and how long each is.
///
/// Month and year have no fixed length in minutes and are left `None` on
/// purpose: converting them here would bake in an approximation that the
/// recurrence machinery (§3.4) has to get exactly right.
pub const UNITS: &[Word] = &[
    n("minute", 1),
    n("minutes", 1),
    n("min", 1),
    n("mins", 1),
    n("hour", 60),
    n("hours", 60),
    n("hr", 60),
    n("hrs", 60),
    n("day", 24 * 60),
    n("days", 24 * 60),
    n("week", 7 * 24 * 60),
    n("weeks", 7 * 24 * 60),
    n("wk", 7 * 24 * 60),
    n("wks", 7 * 24 * 60),
    w("month"),
    w("months"),
    w("mo"),
    w("mos"),
    w("year"),
    w("years"),
    w("yr"),
    w("yrs"),
    w("quarter"),
    w("quarters"),
];

/// Words that are a recurrence all by themselves.
pub const FREQ_WORDS: &[Word] = &[
    w("daily"),
    w("nightly"),
    w("weekly"),
    w("monthly"),
    w("quarterly"),
    w("yearly"),
    w("annually"),
    w("hourly"),
];

/// What opens a recurrence.
pub const REC_HEADS: &[Word] = &[w("every"), w("each")];

/// Sets of days a recurrence can name.
pub const DAY_SETS: &[Word] = &[w("weekday"), w("weekdays"), w("weekend"), w("weekends")];

/// Which one — next Tuesday, this quarter, last week.
pub const DETERMINERS: &[Word] = &[w("next"), w("this"), w("last"), w("coming"), w("upcoming")];

/// Words that give what follows them permission to be temporal.
///
/// None of these means anything on its own. "meet me at the pub" contains a
/// marker and no time, and the guards in §3.1.5 are what keep it that way.
pub const MARKERS: &[Word] = &[
    // `in`, as in "in 3 weeks". The grammar in §3.1.5 uses it as a bare literal
    // in the `anchor` production and no table contained it, so the shape the
    // document lists as a worked example could not be recognised at all. It is
    // a marker like the rest: it means nothing alone and gives what follows it
    // permission to be temporal.
    w("in"),
    w("at"),
    w("on"),
    w("by"),
    w("due"),
    w("from"),
    w("before"),
    w("after"),
    w("starting"),
    w("until"),
];

/// What joins the two ends of a range.
pub const RANGE_OPS: &[Word] = &[
    w("-"),
    w(".."),
    w("to"),
    w("through"),
    w("thru"),
    w("until"),
];

/// Before noon, after noon.
pub const MERIDIEM: &[Word] = &[w("am"), w("pm"), w("a.m."), w("p.m.")];

/// "every *other* week".
pub const INTERVALS: &[Word] = &[w("other")];

/// Words that stop a match dead.
///
/// The first and most valuable guard in §3.1.5: an article in front of a month,
/// a weekday or a bare clock means the word is naming something rather than
/// dating it. This is what makes "the march timeline slipped" and "the 7am
/// problem" — the document's own examples — stay prose.
/// Articles only. `this` and `that` are *not* here, though an earlier draft had
/// them: "this friday" and "that thursday" are both perfectly good dates, and
/// blocking them would have cost more real captures than the prose it saved.
pub const BLOCKERS: &[Word] = &[w("the"), w("a"), w("an")];

/// Verbs that make a line look like a task.
///
/// Scored at 0.40, which is under the floor, so nothing here promotes on its
/// own. Verbs are an open class and any list is wrong at the edges; this one
/// exists so review can offer "task?" on a line that plainly is one, and no
/// further than that.
pub const TASK_VERBS: &[Word] = &[
    w("send"),
    w("call"),
    w("email"),
    w("buy"),
    w("ask"),
    w("book"),
    w("fix"),
    w("write"),
    w("review"),
    w("pay"),
    w("order"),
    w("check"),
    w("ping"),
    w("schedule"),
    w("renew"),
    w("cancel"),
    w("submit"),
    w("file"),
    w("print"),
];

/// Below this, nothing is promoted — §3.1.5.
///
/// The parse becomes a one-line suggestion in review instead. Bare weekdays,
/// vague times of day and every task-verb guess sit under it deliberately:
/// those are the readings most likely to be wrong, and a wrong suggestion costs
/// a glance where a wrong promotion costs a calendar entry.
pub const CONFIDENCE_FLOOR: f32 = 0.75;

/// An explicit marker, and one example of what follows it.
///
/// The accelerant of §3.1.1: always recognised, always beating inference, never
/// required for anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sigil {
    /// The character that opens it.
    pub sigil: char,
    /// What a person would write after it.
    pub example: &'static str,
    /// What it does.
    pub means: &'static str,
}

/// Every sigil there is.
pub const SIGILS: &[Sigil] = &[
    Sigil {
        sigil: '#',
        example: "tag",
        means: "file it under something",
    },
    Sigil {
        sigil: '@',
        example: "person",
        means: "who it concerns",
    },
    Sigil {
        sigil: '>',
        example: "date",
        means: "when it is due",
    },
    Sigil {
        sigil: '!',
        example: "p1",
        means: "how much it matters",
    },
    Sigil {
        sigil: '~',
        example: "30m",
        means: "how long it takes",
    },
    Sigil {
        sigil: '^',
        example: "project",
        means: "what it belongs to",
    },
];

/// Every class, in the order a person reading them would want.
///
/// The overlay renders the vocabulary from this rather than naming each table
/// itself, so a class added here appears there without the window being touched.
pub const CLASSES: &[(&str, &[Word])] = &[
    ("weekday", WEEKDAYS),
    ("month", MONTHS),
    ("day_word", DAY_WORDS),
    ("tod_word", TIME_WORDS),
    ("unit", UNITS),
    ("freq_word", FREQ_WORDS),
    ("rec_head", REC_HEADS),
    ("day_set", DAY_SETS),
    ("determiner", DETERMINERS),
    ("marker", MARKERS),
    ("range_op", RANGE_OPS),
    ("meridiem", MERIDIEM),
    ("interval", INTERVALS),
    ("task_verb", TASK_VERBS),
    ("blocker", BLOCKERS),
];

/// The grammar, as specified in `DESIGN.md` §3.1.5.
///
/// Held as text because its only consumer is something that shows it to a
/// person. The parser M2 builds will be written against the same section, and
/// this string is what it has to agree with.
pub const EBNF: &str = "\
capture     = { sigil | temporal | word } ;

sigil       = \"#\" slug | \"@\" slug | \">\" ( iso_date | day_ref )
            | \"!\" \"p\" digit | \"~\" number unit | \"^\" slug ;

temporal    = recurrence | range | point ;

recurrence  = rec_head [ interval | number ] rec_body [ [ \"at\" ] clock ]
            | freq_word [ [ \"at\" ] clock ] ;
rec_body    = weekday | day_set | unit ;

range       = month day_num range_op day_num [ year ]
            | point range_op point ;

point       = [ marker ] anchor [ [ \"at\" ] clock ] ;
anchor      = iso_date
            | month day_num [ year ]
            | day_num month [ year ]
            | weekday
            | day_word
            | quarter
            | \"in\" number unit
            | determiner ( weekday | unit | quarter ) ;

clock       = hour [ \":\" minute ] [ meridiem ]
            | tod_word ;";

/// What stops a match, and the example each rule exists for.
///
/// These are not in the grammar on purpose: they are about what surrounds a
/// match rather than its structure, and §3.1.3 makes them the more important
/// half — a false positive is worse than a false negative here.
pub const GUARDS: &[&str] = &[
    "an article before a month, weekday or clock kills it — \"the march timeline slipped\"",
    "a bare number is never a time; it needs a colon, a meridiem or a marker — \"budget 40k\"",
    "a following noun kills a bare weekday or month — \"friday deploy\"",
    "a match starts at the capture head, at a comma, or after a marker — never mid-clause",
    "a sigil pre-empts inference entirely; the inference is not even recorded",
];

/// The confidence each shape scores, and what it is.
///
/// Everything below [`CONFIDENCE_FLOOR`] becomes a one-line suggestion in
/// review rather than a promotion.
pub const BANDS: &[(&str, &str)] = &[
    (
        "1.00",
        "a sigil — not scored, never overridden by inference",
    ),
    (
        "0.90",
        "a complete date or datetime behind a marker: on aug 14, by friday 9am",
    ),
    (
        "0.85",
        "two vocabulary items adjacent: thursday 2pm, aug 1-14, in 3 weeks",
    ),
    (
        "0.80",
        "a recurrence head with a body: every weekday at 7, daily",
    ),
    (
        "0.70",
        "a bare weekday or day_word, no clock: dentist thursday",
    ),
    (
        "0.60",
        "a tod_word standing in for a clock: tomorrow morning",
    ),
    (
        "0.80",
        "a task verb and a bare day: call the dentist tomorrow",
    ),
    (
        "0.40",
        "a task verb and no temporal expression: send sarah the doc",
    ),
];

/// One shape the grammar recognises, written the way someone would type it.
///
/// These are for showing, not for parsing — §3.1.1 settles that the language is
/// English and there is nothing to get right, so what a surface can usefully
/// display is examples rather than rules. Each is a real production from the
/// grammar in the module docs.
pub const SHAPES: &[&str] = &["friday 2pm", "every weekday at 7", "aug 1-14", "in 3 weeks"];

/// The people and projects a capture names.
///
/// `@sarah`, `^acme-migration` — the sigils that denote something with a page
/// of its own (§4.3), returned as `(sigil, value)` in the order written.
///
/// Separate from [`recognize`] on purpose. That answers "when is this", needs a
/// clock and a zone to do it, and returns a reading that may be wrong. This
/// answers "what does it mention", which is exact: a sigil is written by hand
/// and is never inferred, so there is nothing here to be unsure about and no
/// reason to make a caller supply a clock to find out.
///
/// `#tag` is here too, and that is not a contradiction of the `tag` event kind.
/// The two are different facts: a tag *event* is an OR-Set membership somebody
/// or something added after the fact and can retract (§3.7), while a `#` in the
/// text is part of what the capture says and changes only when the text does. A
/// note may carry either or both.
///
/// Leaving it out was the first version of this, and it meant typing `#reading`
/// in a capture did nothing at all: the recognizer collected the sigil and
/// dropped it, so the one organising axis somebody writes by hand was the one
/// that never appeared anywhere.
pub fn mentions(text: &str) -> alloc::vec::Vec<(char, alloc::string::String)> {
    use alloc::string::ToString;
    tokenize(text)
        .into_iter()
        .filter_map(|t| match t.class {
            Class::Sigil(c) if c == '@' || c == '^' || c == '#' => {
                t.sigil_value().map(|v| (c, v.to_string()))
            }
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every table, for the checks that apply to all of them.
    fn tables() -> [(&'static str, &'static [Word]); 15] {
        [
            ("WEEKDAYS", WEEKDAYS),
            ("MONTHS", MONTHS),
            ("DAY_WORDS", DAY_WORDS),
            ("TIME_WORDS", TIME_WORDS),
            ("UNITS", UNITS),
            ("FREQ_WORDS", FREQ_WORDS),
            ("REC_HEADS", REC_HEADS),
            ("DAY_SETS", DAY_SETS),
            ("DETERMINERS", DETERMINERS),
            ("MARKERS", MARKERS),
            ("RANGE_OPS", RANGE_OPS),
            ("MERIDIEM", MERIDIEM),
            ("INTERVALS", INTERVALS),
            ("TASK_VERBS", TASK_VERBS),
            ("BLOCKERS", BLOCKERS),
        ]
    }

    #[test]
    fn every_word_is_lowercase_and_not_blank() {
        // The parser will fold input to lowercase before looking anything up,
        // so an uppercase entry here is an entry that can never match.
        for (name, table) in tables() {
            for word in table {
                assert!(!word.word.is_empty(), "{name} has an empty word");
                assert_eq!(
                    word.word,
                    word.word.to_lowercase(),
                    "{name} has {:?}, which is not lowercase",
                    word.word
                );
            }
        }
    }

    #[test]
    fn no_table_repeats_itself() {
        for (name, table) in tables() {
            for (i, a) in table.iter().enumerate() {
                for b in &table[i + 1..] {
                    assert_ne!(a.word, b.word, "{name} lists {:?} twice", a.word);
                }
            }
        }
    }

    #[test]
    fn a_word_means_one_thing() {
        // A word in two classes is a word the parser has to disambiguate, and
        // §3.1.5 buys its determinism by not having any. `until` is the single
        // exception and is asserted below rather than waived silently.
        for (i, (name_a, a)) in tables().iter().enumerate() {
            for (name_b, b) in &tables()[i + 1..] {
                for x in *a {
                    for y in *b {
                        if x.word == y.word {
                            assert_eq!(
                                (x.word, *name_a, *name_b),
                                ("until", "MARKERS", "RANGE_OPS"),
                                "{:?} is in both {name_a} and {name_b}",
                                x.word
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_blocker_is_never_also_temporal() {
        // The guard that kills a match cannot also be a word that opens one.
        // `this` sat in both lists in an earlier draft, which would have made
        // "this friday" unparseable — the cross-table check above now catches
        // that shape of mistake, and this states the rule it is protecting.
        for blocker in BLOCKERS {
            for (name, table) in tables() {
                if name == "BLOCKERS" {
                    continue;
                }
                for word in table {
                    assert_ne!(
                        blocker.word, word.word,
                        "{:?} both blocks a match and is one, via {name}",
                        blocker.word
                    );
                }
            }
        }
    }

    #[test]
    fn until_is_the_only_word_in_two_places() {
        // It opens a point ("until friday") and it joins a range ("mon until
        // fri"). Which one it is depends on what follows, so the parser has to
        // decide it positionally — recorded here so that stays deliberate.
        assert!(MARKERS.iter().any(|w| w.word == "until"));
        assert!(RANGE_OPS.iter().any(|w| w.word == "until"));
    }

    #[test]
    fn times_of_day_land_inside_a_day() {
        for word in TIME_WORDS {
            let minutes = word.minutes.expect("a time of day is a time");
            assert!(minutes < 24 * 60, "{:?} is not a time", word.word);
        }
    }

    #[test]
    fn the_units_that_can_be_measured_are() {
        // Month and year are deliberately unmeasured: neither has a fixed
        // length, and approximating one here would put the error somewhere the
        // recurrence machinery cannot see it.
        for word in UNITS {
            let measurable = !matches!(
                word.word,
                "month"
                    | "months"
                    | "mo"
                    | "mos"
                    | "year"
                    | "years"
                    | "yr"
                    | "yrs"
                    | "quarter"
                    | "quarters"
            );
            assert_eq!(
                word.minutes.is_some(),
                measurable,
                "{:?} disagrees with itself about being measurable",
                word.word
            );
        }
    }

    #[test]
    fn each_sigil_is_used_once() {
        for (i, a) in SIGILS.iter().enumerate() {
            for b in &SIGILS[i + 1..] {
                assert_ne!(a.sigil, b.sigil, "{:?} opens two sigils", a.sigil);
            }
        }
    }

    #[test]
    fn a_sigil_is_punctuation_a_capture_would_not_start_with() {
        // Every one has to be a character that is unambiguous at the head of a
        // word, or the accelerant becomes a trap for ordinary text.
        for sigil in SIGILS {
            assert!(
                sigil.sigil.is_ascii_punctuation(),
                "{:?} is not punctuation",
                sigil.sigil
            );
            assert!(!sigil.example.is_empty());
            assert!(!sigil.means.is_empty());
        }
    }

    #[test]
    fn the_floor_leaves_room_on_both_sides() {
        // §3.1.5 puts shapes at 0.90 down to 0.40 around it. A floor at either
        // extreme would make the suggestion tier or the promotion tier empty.
        const { assert!(CONFIDENCE_FLOOR > 0.40) };
        const { assert!(CONFIDENCE_FLOOR < 0.90) };
    }

    #[test]
    fn the_class_list_covers_every_table() {
        // The overlay renders from CLASSES, so a table missing from it is a
        // part of the language that exists and is never shown to anyone.
        for (name, table) in tables() {
            let listed = CLASSES.iter().any(|(_, c)| core::ptr::eq(*c, table));
            assert!(listed, "{name} is a table that CLASSES does not list");
        }
        assert_eq!(CLASSES.len(), tables().len());
    }

    #[test]
    fn the_bands_bracket_the_floor() {
        // A band list entirely above or below the floor would mean one of the
        // two tiers §3.1.3 describes is unreachable.
        let scores: alloc::vec::Vec<f32> = BANDS
            .iter()
            .map(|(s, _)| s.parse().expect("a band is a number"))
            .collect();
        assert!(
            scores.iter().any(|s| *s >= CONFIDENCE_FLOOR),
            "nothing promotes"
        );
        assert!(
            scores.iter().any(|s| *s < CONFIDENCE_FLOOR),
            "nothing suggests"
        );
    }

    #[test]
    fn the_grammar_and_the_guards_are_not_empty() {
        assert!(
            EBNF.contains("temporal"),
            "the grammar lost its middle tier"
        );
        assert!(EBNF.contains("sigil"), "the grammar lost tier 1");
        assert_eq!(GUARDS.len(), 5, "§3.1.5 states five guards");
    }

    #[test]
    fn every_shape_is_built_from_words_that_exist() {
        // The examples shown to a person have to be examples the recognizer
        // would actually accept, or the surface is teaching a language the
        // parser does not speak.
        let known = |word: &str| {
            tables()
                .iter()
                .any(|(_, t)| t.iter().any(|w| w.word == word))
                || word.chars().all(|c| c.is_ascii_digit())
                || word.ends_with("pm")
                || word.ends_with("am")
                || word.contains('-')
                || word == "in"
        };
        for shape in SHAPES {
            for word in shape.split_whitespace() {
                assert!(known(word), "{shape:?} uses {word:?}, which is in no table");
            }
        }
    }
}
