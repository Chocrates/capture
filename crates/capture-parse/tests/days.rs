//! Which day a weekday means, when the words leave room for two.
//!
//! Found in use on 2026-10-13. "therapy next thursday at 4pm", written on a
//! Tuesday, landed on the 15th; the person meant the 22nd. Corrected to
//! "therapy Thursday 2026-10-22 at 4pm", it landed on the 15th again, all day:
//! the weekday was read and the date and the 4pm after it were not.

use capture_parse::{Shape, When, answer_day, recognize, unclear_day};
use capture_time::{FloatingDate, Zone};

/// Noon in Chicago on the day named.
const TUE_13: i64 = 1_791_910_800_000;
const THU_15: i64 = 1_792_083_600_000;
const FRI_16: i64 = 1_792_170_000_000;
const SUN_18: i64 = 1_792_342_800_000;

fn chicago() -> Zone {
    Zone::new("America/Chicago").unwrap()
}

fn date(s: &str) -> FloatingDate {
    FloatingDate::parse(s).unwrap()
}

/// The local date and clock a reading landed on, or the date alone.
fn landed(text: &str, now: i64) -> String {
    let r = recognize(text, now, chicago());
    match r.shape {
        Shape::Point(When::Moment(ms)) => {
            let (h, m) = capture_time::civil_clock(ms, chicago());
            format!("{} {h:02}:{m:02}", capture_time::civil_date(ms, chicago()))
        }
        Shape::Point(When::Date(d)) => d.to_string(),
        other => format!("{other:?}"),
    }
}

fn promotes(text: &str, now: i64) -> bool {
    recognize(text, now, chicago()).confidence >= (capture_parse::CONFIDENCE_FLOOR * 100.0) as u32
}

// ------------------------------------------------------------ weekday + date

#[test]
fn the_therapy_note_is_the_day_and_time_it_says() {
    let text = "therapy Thursday 2026-10-22 at 4pm";
    assert_eq!(landed(text, TUE_13), "2026-10-22 16:00");
    assert!(promotes(text, TUE_13), "an agreeing weekday is not doubt");
    assert_eq!(unclear_day(text, TUE_13, chicago()), None);
}

#[test]
fn a_weekday_and_a_date_that_agree_are_one_day_however_spelled() {
    for text in [
        "therapy thursday oct 22 at 4pm",
        "therapy Thursday, 22 October 4pm",
        "therapy thu 2026-10-22 4pm",
        "therapy oct 22 thursday at 4pm",
        "therapy oct 22, thursday 4pm",
    ] {
        assert_eq!(landed(text, TUE_13), "2026-10-22 16:00", "{text}");
        assert!(promotes(text, TUE_13), "{text}");
    }
    assert_eq!(landed("therapy thursday oct 22", TUE_13), "2026-10-22");
}

#[test]
fn a_weekday_and_a_date_that_disagree_are_asked() {
    // The 23rd is a Friday.
    let text = "therapy Thursday 2026-10-23 at 4pm";
    let r = recognize(text, TUE_13, chicago());
    assert_eq!(r.shape, Shape::Nothing, "neither is guessed");
    let u = unclear_day(text, TUE_13, chicago()).expect("asked");
    assert_eq!(u.choices, [date("2026-10-22"), date("2026-10-23")]);
    assert_eq!(&text[u.start..u.end], "Thursday 2026-10-23");
}

// ------------------------------------------------------------ next and last

#[test]
fn next_thursday_early_in_the_week_is_asked() {
    let text = "therapy next thursday at 4pm";
    assert_eq!(recognize(text, TUE_13, chicago()).shape, Shape::Nothing);
    let u = unclear_day(text, TUE_13, chicago()).expect("asked");
    assert_eq!(u.choices, [date("2026-10-15"), date("2026-10-22")]);
    assert_eq!(&text[u.start..u.end], "next thursday");
}

#[test]
fn next_thursday_once_this_weeks_has_gone_is_the_coming_one() {
    assert_eq!(
        landed("therapy next thursday at 4pm", FRI_16),
        "2026-10-22 16:00"
    );
    assert_eq!(
        landed("therapy next thursday at 4pm", SUN_18),
        "2026-10-22 16:00"
    );
    assert_eq!(
        unclear_day("therapy next thursday", FRI_16, chicago()),
        None
    );
}

#[test]
fn next_thursday_said_on_a_thursday_is_a_week_away() {
    assert_eq!(
        landed("therapy next thursday at 4pm", THU_15),
        "2026-10-22 16:00"
    );
}

#[test]
fn this_thursday_said_on_a_thursday_is_today() {
    assert_eq!(
        landed("therapy this thursday at 4pm", THU_15),
        "2026-10-15 16:00"
    );
    assert_eq!(
        landed("therapy thursday at 4pm", THU_15),
        "2026-10-15 16:00"
    );
}

#[test]
fn last_thursday_is_behind_not_ahead() {
    assert_eq!(
        landed("saw the dentist last thursday", TUE_13),
        "2026-10-08"
    );
    assert_eq!(
        landed("saw the dentist last thursday", THU_15),
        "2026-10-08"
    );
}

// ---------------------------------------------------------------- answering

#[test]
fn an_answer_rewrites_the_day_in_words_that_read_back_as_it() {
    let text = "therapy next thursday at 4pm";
    let later = answer_day(text, TUE_13, chicago(), 1).unwrap();
    assert_eq!(later, "therapy thursday oct 22 at 4pm");
    assert_eq!(landed(&later, TUE_13), "2026-10-22 16:00");

    let sooner = answer_day(text, TUE_13, chicago(), 0).unwrap();
    assert_eq!(sooner, "therapy thursday oct 15 at 4pm");
    assert_eq!(landed(&sooner, TUE_13), "2026-10-15 16:00");

    let text = "therapy Thursday 2026-10-23 at 4pm";
    assert_eq!(
        answer_day(text, TUE_13, chicago(), 1).unwrap(),
        "therapy friday oct 23 at 4pm"
    );
    assert_eq!(answer_day("nothing to ask", TUE_13, chicago(), 0), None);
    assert_eq!(
        answer_day(text, TUE_13, chicago(), 2),
        None,
        "two choices only"
    );
}

#[test]
fn an_answer_in_another_year_is_written_in_full() {
    // Captured 13 Oct; "thursday oct 1" would read as next year's.
    let text = "dinner thursday 2026-10-02";
    let u = unclear_day(text, TUE_13, chicago()).expect("the 2nd is a Friday");
    assert_eq!(u.choices, [date("2026-10-01"), date("2026-10-02")]);
    let answered = answer_day(text, TUE_13, chicago(), 0).unwrap();
    assert_eq!(answered, "dinner thursday 2026-10-01");
    assert_eq!(landed(&answered, TUE_13), "2026-10-01");
}
