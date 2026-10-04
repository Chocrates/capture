//! What the recognizer makes of real captures.
//!
//! Two kinds of test here and they are not interchangeable. The guards get the
//! document's own counter-examples, because §3.1.3 names false positives as the
//! worse error and those are the sentences it names. The shapes get the five
//! worked examples from §3.1, because those are what the design says the
//! recognizer is *for* — and until now nothing had ever checked that the
//! vocabulary written in §3.1.5 can actually express them.

use capture_parse::{CONFIDENCE_FLOOR, Shape, When, recognize};
use capture_time::Zone;

/// A fixed moment to resolve against: Monday 1 June 2026, 09:00 in Denver.
const NOW: i64 = 1_780_326_000_000;

fn denver() -> Zone {
    Zone::new("America/Denver").unwrap()
}

fn read(text: &str) -> capture_parse::Recognition {
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
    assert_eq!(r.shape, Shape::Task { due: None });
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
            let d = capture_time::civil_date(ms, denver());
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
    for w in capture_parse::WEEKDAYS {
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
    for w in capture_parse::MONTHS {
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
    for w in capture_parse::FREQ_WORDS {
        assert!(
            matches!(read(w.word).shape, Shape::Recurring { .. }),
            "{:?} is in FREQ_WORDS and did not parse as a recurrence",
            w.word
        );
    }
}

#[test]
fn every_day_word_resolves_to_a_day() {
    for w in capture_parse::DAY_WORDS {
        assert!(
            matches!(read(w.word).shape, Shape::Point(_)),
            "{:?} is in DAY_WORDS and did not resolve",
            w.word
        );
    }
}

#[test]
fn every_task_verb_is_seen_at_the_head() {
    for w in capture_parse::TASK_VERBS {
        let text = alloc_format(w.word, "the thing");
        assert_eq!(
            read(&text).shape,
            Shape::Task { due: None },
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

// ------------------------------------------------- found on real captures ---
//
// Task 11.3. Each of these is a capture that was actually made, that the
// recognizer got wrong, and that forced a change. §3.1.5's bands and
// productions were reasoned rather than measured, and this is the only honest
// way to find out where the reasoning was thin — a grammar that has only ever
// been tested against its own examples will pass every one of them.

#[test]
fn a_bare_clock_means_today() {
    // "meeting at 14:00" read as nothing. The `point` production requires an
    // anchor and a bare clock is not one, so about the most ordinary capture
    // there is had no production at all.
    match read("meeting at 14:00").shape {
        Shape::Point(When::Moment(ms)) => {
            let d = capture_time::civil_date(ms, denver());
            let (h, m) = capture_time::civil_clock(ms, denver());
            assert_eq!((d.year, d.month, d.day), (2026, 6, 1), "not today");
            assert_eq!((h, m), (14, 0));
        }
        other => panic!("meeting at 14:00 -> {other:?}"),
    }
    assert!(promotes("meeting at 14:00"));
}

#[test]
fn a_bare_clock_is_still_refused_behind_an_article() {
    // The guard has to survive the new production, or "the 7am problem" starts
    // becoming a calendar entry — §3.1.3's own counter-example.
    assert_eq!(read("the 7am problem").shape, Shape::Nothing);
}

#[test]
fn an_offset_shorter_than_a_day_is_a_moment() {
    // "go to the gas station in 10 minutes" read as nothing: `in N unit` only
    // resolved days and longer, and rounding ten minutes to a date would turn
    // a reminder into an all-day entry.
    match read("go to the gas station in 10 minutes").shape {
        Shape::Point(When::Moment(ms)) => {
            assert_eq!(ms, NOW + 10 * 60_000);
        }
        other => panic!("in 10 minutes -> {other:?}"),
    }

    match read("call back in 2 hours").shape {
        Shape::Point(When::Moment(ms)) => assert_eq!(ms, NOW + 120 * 60_000),
        other => panic!("in 2 hours -> {other:?}"),
    }
}

#[test]
fn a_forward_marker_never_resolves_into_the_past() {
    // "revisit in Q2", said in June, resolved to the April that had already
    // been. A forward marker pointing backwards is a review prompt that can
    // never come due — and forward markers are review's first selection tier,
    // so it would have sat there being ignored.
    match read("budget 40k, revisit in Q2").shape {
        Shape::Point(When::Date(d)) => {
            assert!(
                (d.year, d.month) > (2026, 6),
                "resolved to {d}, which is not ahead of the capture"
            );
        }
        other => panic!("revisit in Q2 -> {other:?}"),
    }
}

#[test]
fn the_grammar_version_moved_with_the_grammar() {
    // The version is what makes a fix reach notes captured before it. Changing
    // a production without bumping it would leave every existing note parsed by
    // the old rules for ever, which is the whole failure the version prevents.
    const { assert!(capture_parse::GRAMMAR_VERSION >= 4) };
}

// ---------------------------------------------------------------------------
// A verb and a day compose. The captures that forced the band, pinned per
// §3.1.5's rule that a band moves only when a real capture demands it.
// ---------------------------------------------------------------------------

#[test]
fn a_task_verb_in_front_of_a_day_is_a_due_date() {
    // The defect this fixes: the two tiers did not compose, so the day was read
    // alone and the verb was discarded. "Call the dentist tomorrow" became an
    // all-day event on Tuesday — and by §3.6's table that is wrong, because an
    // event occupies time and calling the dentist does not occupy a Tuesday.
    let r = read("call the dentist tomorrow");
    match r.shape {
        Shape::Task { due: Some(d) } => assert_eq!(format!("{d}"), "2026-06-02"),
        other => panic!("expected a task due tomorrow, got {other:?}"),
    }
    assert!(promotes("call the dentist tomorrow"));
}

#[test]
fn the_day_may_sit_behind_a_marker() {
    let r = read("email sarah about the invoice on friday");
    match r.shape {
        Shape::Task { due: Some(d) } => assert_eq!(format!("{d}"), "2026-06-05"),
        other => panic!("expected a task due friday, got {other:?}"),
    }
}

#[test]
fn a_clock_makes_an_event_whatever_verb_introduced_it() {
    // The discriminator is time rather than wording (§3.6). A call you are on
    // at three occupies three o'clock, so it is an event — and reading it as a
    // task due today would lose the only part of it that has to be kept.
    match read("call sarah at 3pm").shape {
        Shape::Point(When::Moment(_)) => {}
        other => panic!("expected a moment, got {other:?}"),
    }
}

#[test]
fn a_due_date_never_becomes_an_instant() {
    // §3.6: "due Friday" is not 00:00 UTC on Friday. The type makes the mistake
    // unrepresentable — a `FloatingDate` has no conversion to an instant — and
    // this is here so that a later convenience method cannot quietly add one.
    match read("pay the invoice on friday").shape {
        Shape::Task { due: Some(_) } => {}
        other => panic!("expected a floating due date, got {other:?}"),
    }
}

#[test]
fn a_verb_with_no_day_still_only_suggests() {
    // Unchanged and deliberately so. Verbs are an open class and any list is
    // wrong at the edges, so the verb alone stays below the floor: it offers
    // "task?" and applies nothing.
    let r = read("call the dentist about the crown");
    assert_eq!(r.shape, Shape::Task { due: None });
    assert!(!promotes("call the dentist about the crown"));
    assert!(r.confidence > 0, "it is a suggestion, not silence");
}

#[test]
fn a_day_with_no_verb_is_still_a_day() {
    // The composition must not swallow the tier it composes with. "Dentist
    // thursday" is an appointment nobody phrased as a task, and it keeps its
    // own band — below the floor, where §3.1.5 put it.
    match read("dentist thursday").shape {
        Shape::Point(When::Date(_)) => {}
        other => panic!("expected a bare day, got {other:?}"),
    }
    assert!(!promotes("dentist thursday"));
}

#[test]
fn the_guards_still_hold_against_a_verb() {
    // A task verb must not become a licence to match loosely. These are the
    // document's own counter-examples with a verb in front, and every one of
    // them must still find nothing to be due.
    for text in [
        "check the march timeline",
        "fix the 7am problem",
        "review the budget 40k",
    ] {
        let r = read(text);
        assert_eq!(
            r.shape,
            Shape::Task { due: None },
            "{text:?} found a date the guards should have killed"
        );
    }
}

#[test]
fn an_offset_behind_a_verb_is_a_nudge_rather_than_a_due_date() {
    // Found by the band test above rather than by reasoning, and it is the
    // distinction that keeps the composition honest. "Check back in 3 weeks" is
    // not something you are overdue on come the 23rd — §3.1's own worked
    // example reads "revisit in Q2" as a note with a review prompt, not a task,
    // and `check` being in the verb list must not change that.
    match read("check back in 3 weeks").shape {
        Shape::Point(When::Date(d)) => {
            assert_eq!((d.year, d.month, d.day), (2026, 6, 22));
        }
        other => panic!("an offset became {other:?}"),
    }
    assert_eq!(read("check back in 3 weeks").confidence, 85);
}

#[test]
fn a_verb_does_not_turn_a_series_into_a_task() {
    // Only `point` sees the verb, and this is why. A recurring obligation is a
    // rule, and being introduced by a verb does not make it one thing to do.
    match read("review the docs every friday").shape {
        Shape::Recurring { .. } => {}
        other => panic!("a series became {other:?}"),
    }
}

#[test]
fn a_capture_reports_who_and_what_it_names() {
    // The entity layer's whole input. A sigil is written by hand and never
    // inferred, so this is exact where the rest of the recognizer is a reading.
    let m = capture_parse::mentions("kickoff with @sarah on ^acme-migration #client");
    assert_eq!(
        m,
        [
            ('@', "sarah".to_string()),
            ('^', "acme-migration".to_string()),
            ('#', "client".to_string())
        ],
        "a sigil written in the text is part of what the note says"
    );
}

#[test]
fn a_bare_sigil_names_nobody() {
    assert!(capture_parse::mentions("an @ sign and a ^ caret").is_empty());
}

#[test]
fn an_email_address_is_not_a_person() {
    // `@` in the middle of a word is not a sigil — a sigil starts a token.
    // Otherwise every address in a capture invents a person page.
    assert!(capture_parse::mentions("mail sarah@example.com about it").is_empty());
}

// ------------------------------------------------- dates written exactly ---
//
// The more precisely a date was written, the worse it was read. All three of
// these were found by trying to put one of each kind on a phone.

#[test]
fn a_verb_and_a_sigil_date_is_a_task_due() {
    // The worst of the three: `recognize` returned at the sigil path before
    // `head_is_verb` was computed, so the most explicit way to write a due date
    // produced an *event* — a thing you attend rather than a thing you owe,
    // which is the wrong half of §3.6's table.
    match read("pay the invoice >2026-09-20").shape {
        Shape::Task { due: Some(d) } => assert_eq!(format!("{d}"), "2026-09-20"),
        other => panic!("expected a task due that day, got {other:?}"),
    }
}

#[test]
fn a_sigil_date_with_no_verb_is_still_an_event() {
    // The composition reads the verb; it does not invent one.
    match read("dentist >2026-09-20").shape {
        Shape::Point(When::Date(d)) => assert_eq!(format!("{d}"), "2026-09-20"),
        other => panic!("expected an event on that day, got {other:?}"),
    }
}

#[test]
fn a_sigil_date_is_still_fully_confident() {
    assert_eq!(read("pay the invoice >2026-09-20").confidence, 100);
    assert_eq!(read("dentist >2026-09-20").confidence, 100);
}

#[test]
fn a_date_written_in_full_is_read_in_prose() {
    // A hyphen between digits is the range operator, so `2026-09-20` used to be
    // split into three numbers and two operators and never reached the
    // classifier whole. The one unambiguous form was the one that was invisible.
    match read("dentist 2026-09-20").shape {
        Shape::Point(When::Date(d)) => assert_eq!(format!("{d}"), "2026-09-20"),
        other => panic!("expected that day, got {other:?}"),
    }
    match read("pay the invoice on 2026-09-20").shape {
        Shape::Task { due: Some(d) } => assert_eq!(format!("{d}"), "2026-09-20"),
        other => panic!("expected a task due that day, got {other:?}"),
    }
}

#[test]
fn a_range_is_still_a_range() {
    // The ordering above must not eat `aug 1-14`, which is what the hyphen rule
    // was for in the first place.
    match read("vacation aug 1-14").shape {
        Shape::Range { .. } => {}
        other => panic!("expected a span, got {other:?}"),
    }
}

#[test]
fn only_a_whole_token_that_is_a_real_day_is_a_date() {
    for not_a_date in [
        "dentist 2026-13-45",
        "v2026-09-20 shipped",
        "build 2026-09-201",
        "the 2026-09-20 timeline",
    ] {
        assert!(
            !matches!(read(not_a_date).shape, Shape::Point(When::Date(_))),
            "{not_a_date:?} should not be a date"
        );
    }
}

#[test]
fn a_month_name_is_read_either_way_round() {
    // Day-first is how most of the world writes a date, and reading only one
    // order means the form works silently for some people and not others.
    let a = read("pay the invoice on 20 sep").shape;
    let b = read("pay the invoice on sep 20").shape;
    assert_eq!(a, b, "the two orders named different days");
    match a {
        Shape::Task { due: Some(d) } => assert_eq!(format!("{d}").split('-').nth(2), Some("20")),
        other => panic!("expected a task due on the twentieth, got {other:?}"),
    }
}

#[test]
fn a_count_before_a_unit_is_still_an_offset() {
    // The day-first arm needs a month name after the number, which is what
    // keeps this out: no unit is a month.
    match read("check back in 3 weeks").shape {
        Shape::Point(When::Date(_)) => {}
        other => panic!("expected an offset to a day, got {other:?}"),
    }
}

#[test]
fn an_ambiguous_numeric_date_is_not_guessed_at() {
    // `09/20` and `20/09` are the same characters meaning different days, and
    // nothing in a capture says which convention its author uses. A date read
    // wrongly is worse than one not read: it is confidently off by months.
    for ambiguous in [
        "pay the invoice on 20/09",
        "pay the invoice on 20.09.2026",
        "pay the invoice on 09/20/2026",
    ] {
        assert!(
            !matches!(read(ambiguous).shape, Shape::Task { due: Some(_) }),
            "{ambiguous:?} should name no date"
        );
    }
}
