//! What the recognizer makes of real captures.
//!
//! Two kinds of test here and they are not interchangeable. The guards get the
//! document's own counter-examples, because §3.1.3 names false positives as the
//! worse error and those are the sentences it names. The shapes get the five
//! worked examples from §3.1, because those are what the design says the
//! recognizer is *for* — and until now nothing had ever checked that the
//! vocabulary written in §3.1.5 can actually express them.

use brain_parse::{CONFIDENCE_FLOOR, Shape, When, recognize};
use brain_time::Zone;

/// A fixed moment to resolve against: Monday 1 June 2026, 09:00 in Denver.
const NOW: i64 = 1_780_326_000_000;

fn denver() -> Zone {
    Zone::new("America/Denver").unwrap()
}

fn read(text: &str) -> brain_parse::Recognition {
    recognize(text, NOW, denver())
}

fn promotes(text: &str) -> bool {
    read(text).confidence >= (CONFIDENCE_FLOOR * 100.0) as u32
}

#[test]
fn a_month_inside_a_noun_phrase_is_not_a_date() {
    let r = read("the march timeline slipped");
    assert_eq!(r.shape, Shape::Nothing, "an article did not kill the match");
}

#[test]
fn a_time_behind_an_article_is_prose() {
    assert_eq!(read("the 7am problem").shape, Shape::Nothing);
}

#[test]
fn a_bare_number_is_never_a_time() {
    let r = read("budget 40k, revisit in Q2");
    assert!(
        !matches!(r.shape, Shape::Point(When::Moment(_))),
        "a bare number became a time: {:?}",
        r.shape
    );
}

#[test]
fn a_following_noun_kills_a_bare_weekday() {
    assert_eq!(read("friday deploy").shape, Shape::Nothing);
}

#[test]
fn ordinary_prose_recognises_nothing() {
    for text in [
        "spoke to the team about the release",
        "that was a good idea",
        "the may report needs numbers",
    ] {
        assert_eq!(
            read(text).shape,
            Shape::Nothing,
            "{text:?} matched something"
        );
    }
}

#[test]
fn the_five_worked_examples_parse_as_the_document_says() {
    let r = read("dentist thursday 2pm");
    assert!(
        matches!(r.shape, Shape::Point(When::Moment(_))),
        "dentist thursday 2pm -> {:?}",
        r.shape
    );
    assert!(promotes("dentist thursday 2pm"), "it should promote");

    let r = read("walk the dog every day at 7");
    match r.shape {
        Shape::Recurring { rule, at_minute } => {
            assert_eq!(rule.freq, 0, "expected daily");
            assert_eq!(at_minute, Some(7 * 60), "expected 07:00");
        }
        other => panic!("walk the dog every day at 7 -> {other:?}"),
    }

    let r = read("vacation aug 1-14");
    match r.shape {
        Shape::Range { from, to } => {
            assert_eq!((from.month, from.day), (8, 1));
            assert_eq!((to.month, to.day), (8, 14));
        }
        other => panic!("vacation aug 1-14 -> {other:?}"),
    }

    let r = read("send sarah the migration doc");
    assert_eq!(r.shape, Shape::Task);
    assert!(
        !promotes("send sarah the migration doc"),
        "a task verb must not promote on its own"
    );

    let r = read("budget 40k, revisit in Q2");
    assert!(
        !matches!(r.shape, Shape::Point(When::Moment(_))),
        "budget 40k, revisit in Q2 -> {:?}",
        r.shape
    );
}

#[test]
fn in_three_weeks_lands_three_weeks_out() {
    match read("check back in 3 weeks").shape {
        Shape::Point(When::Date(d)) => {
            assert_eq!((d.year, d.month, d.day), (2026, 6, 22), "1 June + 21 days");
        }
        other => panic!("in 3 weeks -> {other:?}"),
    }
}

#[test]
fn a_weekday_said_on_that_weekday_means_today() {
    match read("monday 9am").shape {
        Shape::Point(When::Moment(ms)) => {
            let d = brain_time::civil_date(ms, denver());
            assert_eq!((d.year, d.month, d.day), (2026, 6, 1));
        }
        other => panic!("monday 9am -> {other:?}"),
    }
}

#[test]
fn every_weekday_at_seven_is_five_days_a_week() {
    match read("every weekday at 7").shape {
        Shape::Recurring { rule, at_minute } => {
            assert_eq!(rule.freq, 1, "weekly");
            assert_eq!(rule.by_day, 0b0001_1111, "Mon to Fri");
            assert_eq!(at_minute, Some(7 * 60));
        }
        other => panic!("every weekday at 7 -> {other:?}"),
    }
}

#[test]
fn every_other_week_carries_its_interval() {
    match read("every other week").shape {
        Shape::Recurring { rule, .. } => {
            assert_eq!(rule.interval, 2);
            assert_eq!(rule.freq, 1);
        }
        other => panic!("every other week -> {other:?}"),
    }
}

#[test]
fn a_sigil_beats_inference_and_suppresses_it() {
    let r = read("dentist thursday >2026-12-25");
    match r.shape {
        Shape::Point(When::Date(d)) => {
            assert_eq!((d.year, d.month, d.day), (2026, 12, 25));
        }
        other => panic!("an explicit date lost to inference: {other:?}"),
    }
    assert_eq!(r.confidence, 100, "a sigil is exact and is not scored");
}

#[test]
fn markers_are_collected_whatever_else_happens() {
    let r = read("call mum #family #urgent @mum ^home !p1 ~30m");
    assert_eq!(r.markers.tags, ["family", "urgent"]);
    assert_eq!(r.markers.people, ["mum"]);
    assert_eq!(r.markers.project.as_deref(), Some("home"));
    assert_eq!(r.markers.priority, Some(1));
    assert_eq!(r.markers.duration_min, Some(30));
}

#[test]
fn a_duration_understands_the_units_it_offers() {
    assert_eq!(read("x ~90m").markers.duration_min, Some(90));
    assert_eq!(read("x ~2h").markers.duration_min, Some(120));
}

#[test]
fn reading_the_same_capture_twice_gives_the_same_answer() {
    for text in [
        "dentist thursday 2pm",
        "every weekday at 7",
        "the march timeline slipped",
        "vacation aug 1-14",
    ] {
        assert_eq!(read(text), read(text), "{text:?}");
    }
}

#[test]
fn a_date_with_no_time_does_not_move_between_zones() {
    let tokyo = Zone::new("Asia/Tokyo").unwrap();
    let a = recognize("vacation aug 1-14", NOW, denver());
    let b = recognize("vacation aug 1-14", NOW, tokyo);
    assert_eq!(a.shape, b.shape);
}

// -------------------------------------------------------------- bands ------

#[test]
fn the_bands_are_the_ones_the_document_states() {
    // §3.1.5's table, as assertions. These decide what silently becomes a
    // calendar entry and what waits as a suggestion, so they are pinned rather
    // than left to whatever the scoring happens to produce.
    for (text, expected) in [
        // 0.90 — a complete date or datetime behind a marker.
        ("on aug 14", 90),
        ("by friday 9am", 90),
        // 0.85 — two vocabulary items adjacent and unambiguous.
        ("dentist thursday 2pm", 85),
        ("check back in 3 weeks", 85),
        // 0.80 — a recurrence head with a body.
        ("every weekday at 7", 80),
        ("walk the dog daily", 80),
        // 0.70 — a bare weekday, no clock.
        ("dentist thursday", 70),
        // 0.60 — a time-of-day word standing in for a clock.
        ("call mum tomorrow morning", 60),
        // 0.40 — a task verb and nothing temporal.
        ("send sarah the migration doc", 40),
    ] {
        assert_eq!(read(text).confidence, expected, "{text:?}");
    }
}

#[test]
fn everything_under_the_floor_is_a_suggestion_and_nothing_else() {
    // The floor is what separates a promotion from a one-line offer in review.
    // Bare weekdays, vague times and every task-verb guess sit under it
    // deliberately: those are the readings most likely to be wrong.
    for text in [
        "dentist thursday",
        "call mum tomorrow morning",
        "send sarah the migration doc",
    ] {
        assert!(!promotes(text), "{text:?} promoted and should not have");
    }
    for text in ["dentist thursday 2pm", "on aug 14", "every weekday at 7"] {
        assert!(promotes(text), "{text:?} did not promote and should have");
    }
}

// ------------------------------------------------- the closed vocabulary ----

#[test]
fn every_weekday_in_the_vocabulary_is_recognised() {
    // The vocabulary is closed precisely so this can be exhaustive rather than
    // representative. A word in the table that the grammar cannot use is a word
    // that does nothing, and nothing else would notice.
    for w in brain_parse::WEEKDAYS {
        let text = alloc_format(w.word, "2pm");
        assert!(
            matches!(read(&text).shape, Shape::Point(When::Moment(_))),
            "{:?} is in WEEKDAYS and did not parse: {:?}",
            w.word,
            read(&text).shape
        );
    }
}

#[test]
fn every_month_in_the_vocabulary_is_recognised() {
    for w in brain_parse::MONTHS {
        let text = alloc_format(w.word, "14");
        assert!(
            matches!(read(&text).shape, Shape::Point(_)),
            "{:?} is in MONTHS and did not parse: {:?}",
            w.word,
            read(&text).shape
        );
    }
}

#[test]
fn every_freq_word_starts_a_recurrence() {
    for w in brain_parse::FREQ_WORDS {
        assert!(
            matches!(read(w.word).shape, Shape::Recurring { .. }),
            "{:?} is in FREQ_WORDS and did not parse as a recurrence",
            w.word
        );
    }
}

#[test]
fn every_day_word_resolves_to_a_day() {
    for w in brain_parse::DAY_WORDS {
        assert!(
            matches!(read(w.word).shape, Shape::Point(_)),
            "{:?} is in DAY_WORDS and did not resolve",
            w.word
        );
    }
}

#[test]
fn every_task_verb_is_seen_at_the_head() {
    for w in brain_parse::TASK_VERBS {
        let text = alloc_format(w.word, "the thing");
        assert_eq!(
            read(&text).shape,
            Shape::Task,
            "{:?} is in TASK_VERBS and was not recognised at the head",
            w.word
        );
    }
}

fn alloc_format(a: &str, b: &str) -> String {
    let mut s = String::from(a);
    s.push(' ');
    s.push_str(b);
    s
}
