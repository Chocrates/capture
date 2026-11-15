//! Sun phrases (alarms-at-sunrise-and-sunset D8): read as times behind `at`,
//! `before` or `after`, and never as the sun named in passing.

use capture_core::body::{SUN_RISE, SUN_SET};
use capture_parse::{CONFIDENCE_FLOOR, Shape, Sun, When, recognize};
use capture_time::Zone;
use proptest::prelude::*;

/// Monday 1 June 2026, 09:00 in Denver.
const NOW: i64 = 1_780_326_000_000;

fn read(text: &str) -> capture_parse::Recognition {
    recognize(text, NOW, Zone::new("America/Denver").unwrap())
}

fn promotes(text: &str) -> bool {
    read(text).confidence >= (CONFIDENCE_FLOOR * 100.0) as u32
}

fn sun(event: u8, offset_min: i16) -> Sun {
    Sun { event, offset_min }
}

#[test]
fn every_day_at_sunrise_is_a_daily_series() {
    let r = read("wake up every day at sunrise");
    match r.shape {
        Shape::SunRecurring { rule, sun: s } => {
            assert_eq!((rule.freq, rule.interval), (0, 1));
            assert_eq!(s, sun(SUN_RISE, 0));
        }
        other => panic!("read as {other:?}"),
    }
    assert!(promotes("wake up every day at sunrise"));
}

#[test]
fn before_sunset_tomorrow_is_one_moment_tomorrow() {
    let r = read("tomorrow 30 min before sunset walk the dog");
    match r.shape {
        Shape::Point(When::Sun(d, s)) => {
            assert_eq!(format!("{d}"), "2026-06-02");
            assert_eq!(s, sun(SUN_SET, -30));
        }
        other => panic!("read as {other:?}"),
    }
    assert!(promotes("tomorrow 30 min before sunset walk the dog"));
}

#[test]
fn weekdays_at_sunrise_is_monday_to_friday() {
    match read("weekdays at sunrise run").shape {
        Shape::SunRecurring { rule, sun: s } => {
            assert_eq!((rule.freq, rule.by_day), (1, 0b0001_1111));
            assert_eq!(s, sun(SUN_RISE, 0));
        }
        other => panic!("read as {other:?}"),
    }
}

#[test]
fn the_other_spellings_read() {
    for (text, day, want) in [
        ("swim at sunset", "2026-06-01", sun(SUN_SET, 0)),
        ("at sunset tomorrow drinks", "2026-06-02", sun(SUN_SET, 0)),
        ("tomorrow at sunrise photos", "2026-06-02", sun(SUN_RISE, 0)),
        (
            "walk the dog 1 hour after sunrise",
            "2026-06-01",
            sun(SUN_RISE, 60),
        ),
        ("lights 2 h before sunset", "2026-06-01", sun(SUN_SET, -120)),
        ("friday at sunset bbq", "2026-06-05", sun(SUN_SET, 0)),
    ] {
        match read(text).shape {
            Shape::Point(When::Sun(d, s)) => {
                assert_eq!((format!("{d}").as_str(), s), (day, want), "{text}")
            }
            other => panic!("{text} read as {other:?}"),
        }
        assert!(promotes(text), "{text}");
    }
    match read("every friday 15 minutes after sunset").shape {
        Shape::SunRecurring { rule, sun: s } => {
            assert_eq!(rule.by_day, 1 << 4);
            assert_eq!(s, sun(SUN_SET, 15));
        }
        other => panic!("read as {other:?}"),
    }
}

#[test]
fn the_sun_named_in_passing_is_not_a_time() {
    for text in [
        "watched the sunset from the pier",
        "sunset boulevard",
        "sunrise over the bay was unreal",
        "the sunset was late",
        "before sunset",
    ] {
        let r = read(text);
        assert_eq!(r.shape, Shape::Nothing, "{text}");
        assert_eq!(r.confidence, 0, "{text}");
    }
}

#[test]
fn a_sunset_marker_is_not_sunday() {
    let r = read("bbq >sunset");
    assert!(
        !matches!(r.shape, Shape::Point(When::Date(_))),
        "read as {:?}",
        r.shape
    );
    assert!(!promotes("bbq >sunset"));
}

#[test]
fn more_than_six_hours_from_the_sun_is_offered_not_written() {
    for text in [
        "call 7 hours after sunset",
        "every day 400 min before sunrise",
    ] {
        let r = read(text);
        assert!(
            matches!(
                r.shape,
                Shape::Point(When::Sun(..)) | Shape::SunRecurring { .. }
            ),
            "{text} read as {:?}",
            r.shape
        );
        assert!(r.confidence > 0, "{text} was not offered");
        assert!(!promotes(text), "{text} promoted unasked");
    }
    // Six hours exactly is still near.
    assert!(promotes("6 hours after sunrise stretch"));
    // Past a day, the number is not a sun offset at all.
    assert_eq!(read("90000 min before sunset").shape, Shape::Nothing);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Every offset in minutes, either way, reads back as written.
    #[test]
    fn an_offset_reads_back_exactly(n in 1u32..=360, before in any::<bool>(), rise in any::<bool>()) {
        let text = format!(
            "x {n} min {} {}",
            if before { "before" } else { "after" },
            if rise { "sunrise" } else { "sunset" }
        );
        let want = sun(if rise { SUN_RISE } else { SUN_SET }, if before { -(n as i16) } else { n as i16 });
        match read(&text).shape {
            Shape::Point(When::Sun(_, s)) => prop_assert_eq!(s, want),
            other => prop_assert!(false, "{} read as {:?}", text, other),
        }
    }
}
