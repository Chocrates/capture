//! Sunrise and sunset against an independent formulation.
//!
//! # Where the reference comes from
//!
//! The values below were computed on 2026-11-14 by the U.S. Naval
//! Observatory's "Almanac for Computers" (1990) sunrise algorithm, as given in
//! Ed Williams' aviation formulary — a separate derivation, with its own
//! constants, from the NOAA/Meeus method `capture_time::sun` implements. The
//! generator (a page of Python) is kept with the change's notes. Each value is
//! UTC minutes after midnight, modulo a day; `None` means no crossing.
//!
//! Copying NOAA's own calculator was the plan (task 1.1). It could not be read
//! from here, so this is agreement between two methods, not against an
//! observation. The real-world check is task 6.3: a week of rings beside a
//! weather app's sunrise.
//!
//! The USNO method is good to about two minutes; agreement within three is
//! what is asserted, and the worst disagreement is printed.

use capture_time::{FloatingDate, SunEvent, Zone, sun};

type Row = (
    &'static str,
    &'static str,
    i32,
    i32,
    (i32, u8, u8),
    Option<f64>,
    Option<f64>,
);

const REFERENCE: &[Row] = &[
    (
        "quito",
        "America/Guayaquil",
        -180700,
        -78467800,
        (2026, 3, 20),
        Some(678.2),
        Some(1404.7),
    ),
    (
        "quito",
        "America/Guayaquil",
        -180700,
        -78467800,
        (2026, 6, 21),
        Some(672.2),
        Some(1399.0),
    ),
    (
        "quito",
        "America/Guayaquil",
        -180700,
        -78467800,
        (2026, 9, 22),
        Some(663.4),
        Some(1389.8),
    ),
    (
        "quito",
        "America/Guayaquil",
        -180700,
        -78467800,
        (2026, 12, 21),
        Some(667.9),
        Some(1396.0),
    ),
    (
        "singapore",
        "Asia/Singapore",
        1352100,
        103819800,
        (2026, 3, 20),
        Some(1389.2),
        Some(675.7),
    ),
    (
        "singapore",
        "Asia/Singapore",
        1352100,
        103819800,
        (2026, 6, 21),
        Some(1380.3),
        Some(672.4),
    ),
    (
        "singapore",
        "Asia/Singapore",
        1352100,
        103819800,
        (2026, 9, 22),
        Some(1374.3),
        Some(660.9),
    ),
    (
        "singapore",
        "Asia/Singapore",
        1352100,
        103819800,
        (2026, 12, 21),
        Some(1381.2),
        Some(664.0),
    ),
    (
        "tokyo",
        "Asia/Tokyo",
        35676200,
        139650300,
        (2026, 3, 20),
        Some(1246.3),
        Some(532.5),
    ),
    (
        "tokyo",
        "Asia/Tokyo",
        35676200,
        139650300,
        (2026, 6, 21),
        Some(1165.7),
        Some(600.3),
    ),
    (
        "tokyo",
        "Asia/Tokyo",
        35676200,
        139650300,
        (2026, 9, 22),
        Some(1228.7),
        Some(519.5),
    ),
    (
        "tokyo",
        "Asia/Tokyo",
        35676200,
        139650300,
        (2026, 12, 21),
        Some(1306.9),
        Some(451.5),
    ),
    (
        "sydney",
        "Australia/Sydney",
        -33868800,
        151209300,
        (2026, 3, 20),
        Some(1197.8),
        Some(487.5),
    ),
    (
        "sydney",
        "Australia/Sydney",
        -33868800,
        151209300,
        (2026, 6, 21),
        Some(1259.8),
        Some(413.7),
    ),
    (
        "sydney",
        "Australia/Sydney",
        -33868800,
        151209300,
        (2026, 9, 22),
        Some(1185.8),
        Some(471.0),
    ),
    (
        "sydney",
        "Australia/Sydney",
        -33868800,
        151209300,
        (2026, 12, 21),
        Some(1120.6),
        Some(545.3),
    ),
    (
        "london",
        "Europe/London",
        51507400,
        -127800,
        (2026, 3, 20),
        Some(364.2),
        Some(1093.0),
    ),
    (
        "london",
        "Europe/London",
        51507400,
        -127800,
        (2026, 6, 21),
        Some(222.9),
        Some(1221.4),
    ),
    (
        "london",
        "Europe/London",
        51507400,
        -127800,
        (2026, 9, 22),
        Some(345.9),
        Some(1079.8),
    ),
    (
        "london",
        "Europe/London",
        51507400,
        -127800,
        (2026, 12, 21),
        Some(483.6),
        Some(953.4),
    ),
    (
        "chicago",
        "America/Chicago",
        41878100,
        -87629800,
        (2026, 3, 20),
        Some(714.3),
        Some(2.6),
    ),
    (
        "chicago",
        "America/Chicago",
        41878100,
        -87629800,
        (2026, 6, 21),
        Some(615.4),
        Some(89.1),
    ),
    (
        "chicago",
        "America/Chicago",
        41878100,
        -87629800,
        (2026, 9, 22),
        Some(697.6),
        Some(1428.2),
    ),
    (
        "chicago",
        "America/Chicago",
        41878100,
        -87629800,
        (2026, 12, 21),
        Some(794.7),
        Some(1342.5),
    ),
    (
        "punta-arenas",
        "America/Punta_Arenas",
        -53163800,
        -70917100,
        (2026, 3, 20),
        Some(644.7),
        Some(1376.7),
    ),
    (
        "punta-arenas",
        "America/Punta_Arenas",
        -53163800,
        -70917100,
        (2026, 6, 21),
        Some(779.5),
        Some(1231.3),
    ),
    (
        "punta-arenas",
        "America/Punta_Arenas",
        -53163800,
        -70917100,
        (2026, 9, 22),
        Some(632.8),
        Some(1361.0),
    ),
    (
        "punta-arenas",
        "America/Punta_Arenas",
        -53163800,
        -70917100,
        (2026, 12, 21),
        Some(492.6),
        Some(70.9),
    ),
    (
        "reykjavik",
        "Atlantic/Reykjavik",
        64146600,
        -21942600,
        (2026, 3, 20),
        Some(449.9),
        Some(1182.5),
    ),
    (
        "reykjavik",
        "Atlantic/Reykjavik",
        64146600,
        -21942600,
        (2026, 6, 21),
        Some(174.9),
        Some(4.0),
    ),
    (
        "reykjavik",
        "Atlantic/Reykjavik",
        64146600,
        -21942600,
        (2026, 9, 22),
        Some(429.6),
        Some(1169.9),
    ),
    (
        "reykjavik",
        "Atlantic/Reykjavik",
        64146600,
        -21942600,
        (2026, 12, 21),
        Some(682.2),
        Some(929.3),
    ),
];

fn minute_of_day(ms: i64) -> f64 {
    (ms.rem_euclid(86_400_000) as f64) / 60_000.0
}

fn apart(a: f64, b: f64) -> f64 {
    let d = (a - b).abs() % 1440.0;
    d.min(1440.0 - d)
}

#[test]
fn agrees_with_the_usno_almanac_within_three_minutes() {
    let mut worst = (0.0, "");
    for (name, tz, lat, lon, (y, m, d), rise, set) in REFERENCE {
        let date = FloatingDate::new(*y, *m, *d).unwrap();
        let zone = Zone::new(tz).unwrap();
        for (event, want) in [(SunEvent::Rise, rise), (SunEvent::Set, set)] {
            let got = sun(date, zone, *lat, *lon, event);
            match (want, got) {
                (Some(w), Ok(ms)) => {
                    let off = apart(*w, minute_of_day(ms));
                    if off > worst.0 {
                        worst = (off, name);
                    }
                    assert!(
                        off <= 3.0,
                        "{name} {y}-{m}-{d} {event:?}: {off:.1} min apart"
                    );
                }
                (None, Err(_)) => {}
                (w, g) => panic!("{name} {y}-{m}-{d} {event:?}: reference {w:?}, got {g:?}"),
            }
        }
    }
    eprintln!("worst disagreement: {:.1} min ({})", worst.0, worst.1);
}

#[test]
fn each_answer_is_on_or_next_to_its_local_date() {
    // The event belongs to the solar day of that date: rise on the date in the
    // local zone; set on it, or just after midnight where summer sunset is late.
    for (name, tz, lat, lon, (y, m, d), _, _) in REFERENCE {
        let date = FloatingDate::new(*y, *m, *d).unwrap();
        let zone = Zone::new(tz).unwrap();
        let start = capture_time::at_local(date, 0, 0, zone).unwrap();
        for event in [SunEvent::Rise, SunEvent::Set] {
            if let Ok(ms) = sun(date, zone, *lat, *lon, event) {
                assert!(
                    ms >= start - 3_600_000 && ms < start + 30 * 3_600_000,
                    "{name} {event:?}"
                );
            }
        }
    }
}

proptest::proptest! {
    #![proptest_config(proptest::prelude::ProptestConfig { cases: 4096, ..Default::default() })]

    /// Anywhere, any day: a whole-minute time near that date, or a polar day.
    /// Never a panic, never a NaN turned into the epoch.
    #[test]
    fn any_place_and_day_gives_a_time_or_says_polar(
        lat in -90_000_000i32..=90_000_000,
        lon in -180_000_000i32..=180_000_000,
        y in 1970i32..2200,
        doy in 0i64..366,
        rise in proptest::bool::ANY,
    ) {
        let zone = Zone::new("UTC").unwrap();
        let date = FloatingDate::new(y, 1, 1).unwrap().plus_days(doy).unwrap();
        let event = if rise { SunEvent::Rise } else { SunEvent::Set };
        if let Ok(ms) = sun(date, zone, lat, lon, event) {
            let midnight = capture_time::at_local(date, 0, 0, zone).unwrap();
            proptest::prop_assert_eq!(ms % 60_000, 0);
            proptest::prop_assert!(ms > midnight - 14 * 3_600_000 && ms < midnight + 38 * 3_600_000,
                "{} vs midnight {}", ms, midnight);
        }
    }
}
