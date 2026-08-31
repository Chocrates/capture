//! Properties of recurrence expansion.
//!
//! These are properties rather than examples because the interesting failures
//! are not ones anybody would think to write down. The unit tests in the crate
//! pin the DST cases and the shapes the design names; these assert the
//! invariants that have to hold for *every* rule, and they are the reason
//! `capture-time` cannot reach a clock — none of this is checkable if expansion
//! can depend on when it ran.

use capture_time::{Freq, Rule, Weekday, Zone, expand};
use chrono::NaiveDate;
use proptest::prelude::*;

const ZONES: &[&str] = &[
    "America/Denver",
    "Europe/London",
    "Asia/Tokyo",
    "Australia/Lord_Howe", // a 30-minute DST shift, which catches assumptions
    "UTC",
];

fn a_rule() -> impl Strategy<Value = Rule> {
    (0usize..4, 1u32..5, 0u8..8, prop::option::of(1u32..20)).prop_map(
        |(freq, interval, days, count)| {
            let freq = [Freq::Daily, Freq::Weekly, Freq::Monthly, Freq::Yearly][freq];
            let mut rule = Rule::new(freq, interval);

            if days != 0 {
                let all = [
                    Weekday::Mon,
                    Weekday::Tue,
                    Weekday::Wed,
                    Weekday::Thu,
                    Weekday::Fri,
                    Weekday::Sat,
                    Weekday::Sun,
                ];
                let chosen: Vec<_> = all
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| days & (1 << i) != 0)
                    .map(|(_, d)| *d)
                    .collect();
                if !chosen.is_empty() {
                    rule = rule.on_days(&chosen);
                }
            }
            if let Some(n) = count {
                rule = rule.count(n);
            }
            rule
        },
    )
}

fn a_start() -> impl Strategy<Value = chrono::NaiveDateTime> {
    (2020i32..2030, 1u32..13, 1u32..29, 0u32..24, 0u32..60).prop_map(|(y, m, d, h, mi)| {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, mi, 0)
            .unwrap()
    })
}

fn a_zone() -> impl Strategy<Value = Zone> {
    (0usize..ZONES.len()).prop_map(|i| Zone::new(ZONES[i]).unwrap())
}

proptest! {
    /// The property the crate exists for.
    #[test]
    fn expansion_is_deterministic(rule in a_rule(), start in a_start(), zone in a_zone()) {
        let from = 1_600_000_000_000i64;
        let to = from + 365 * 24 * 60 * 60 * 1000;
        prop_assert_eq!(
            expand(&rule, start, zone, from, to),
            expand(&rule, start, zone, from, to)
        );
    }

    /// Nothing outside the window it was asked for.
    #[test]
    fn every_occurrence_is_inside_the_window(
        rule in a_rule(), start in a_start(), zone in a_zone(), span in 1i64..400
    ) {
        let from = 1_600_000_000_000i64;
        let to = from + span * 24 * 60 * 60 * 1000;
        for o in expand(&rule, start, zone, from, to) {
            prop_assert!(o.start >= from, "an occurrence fell before the window");
            prop_assert!(o.start < to, "an occurrence fell on or after the window end");
        }
    }

    /// Strictly increasing: two occurrences of one series never share an
    /// instant, which is what makes `(series, original_start)` a name.
    #[test]
    fn occurrences_are_ordered_and_distinct(
        rule in a_rule(), start in a_start(), zone in a_zone()
    ) {
        let from = 1_600_000_000_000i64;
        let to = from + 365 * 24 * 60 * 60 * 1000;
        let occ = expand(&rule, start, zone, from, to);
        for pair in occ.windows(2) {
            prop_assert!(
                pair[0].start < pair[1].start,
                "occurrences are not strictly increasing"
            );
        }
    }

    /// COUNT bounds the series, not the window.
    #[test]
    fn count_is_never_exceeded(rule in a_rule(), start in a_start(), zone in a_zone()) {
        let from = 0i64;
        let to = 2_000_000_000_000i64;
        let occ = expand(&rule, start, zone, from, to);
        if let Some(n) = rule.count {
            prop_assert!(occ.len() <= n as usize, "COUNT was exceeded");
        }
    }

    /// A narrower window is a subset of a wider one.
    ///
    /// This is what makes the rolling horizon safe. If expanding a sub-window
    /// could produce an occurrence the wider expansion did not, then extending
    /// the horizon would change history rather than reveal more of it.
    #[test]
    fn a_sub_window_is_a_subset(
        rule in a_rule(), start in a_start(), zone in a_zone(), cut in 1i64..300
    ) {
        let from = 1_600_000_000_000i64;
        let to = from + 365 * 24 * 60 * 60 * 1000;
        let mid = from + cut * 24 * 60 * 60 * 1000;

        let wide = expand(&rule, start, zone, from, to);
        let narrow = expand(&rule, start, zone, from, mid);

        for o in &narrow {
            prop_assert!(
                wide.contains(o),
                "a narrower window produced an occurrence the wider one did not"
            );
        }
    }

    /// UNTIL is a bound on the instant, and it is inclusive.
    #[test]
    fn until_is_respected(rule in a_rule(), start in a_start(), zone in a_zone(), days in 1i64..200) {
        let from = 1_600_000_000_000i64;
        let until = from + days * 24 * 60 * 60 * 1000;
        let rule = rule.until(until);

        for o in expand(&rule, start, zone, from, until + 10_000_000_000) {
            prop_assert!(o.start <= until, "an occurrence outlived UNTIL");
        }
    }
}
