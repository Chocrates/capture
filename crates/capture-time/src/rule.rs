//! Recurrence: the rule is the truth, the occurrences are not.
//!
//! # Why expansion happens in local wall time
//!
//! "07:00 daily" means 07:00 local on both sides of a DST boundary. Expanding
//! in UTC and converting afterwards would drift the alarm by an hour twice a
//! year — so the rule advances in local days and each result is converted to an
//! instant, never the reverse (`DESIGN.md` §3.2).
//!
//! # The two local times that are not times
//!
//! Advancing in local wall time produces, twice a year, a local time that does
//! not exist and a local time that happens twice. Neither can be left to
//! whatever the conversion happens to do:
//!
//! - **Spring forward** — 02:30 on a day that jumps 02:00 to 03:00. The
//!   occurrence shifts forward to the first valid instant.
//! - **Fall back** — 01:30 on a day where 01:30 happens twice. The first wins.
//!
//! Both are the RFC 5545 conventions. Both are recorded on the occurrence
//! rather than applied silently, because §4.4 draws a marker from it: a time
//! that moves on its own with no explanation is exactly the kind of thing that
//! makes someone stop trusting the whole system.

use alloc::vec::Vec;

use chrono::{Datelike, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Timelike};

use crate::Zone;

/// How often a series repeats.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// A day of the week, as `BYDAY` names them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    fn bit(self) -> u8 {
        1 << (self as u8)
    }

    fn of(d: NaiveDate) -> Weekday {
        match d.weekday() {
            chrono::Weekday::Mon => Weekday::Mon,
            chrono::Weekday::Tue => Weekday::Tue,
            chrono::Weekday::Wed => Weekday::Wed,
            chrono::Weekday::Thu => Weekday::Thu,
            chrono::Weekday::Fri => Weekday::Fri,
            chrono::Weekday::Sat => Weekday::Sat,
            chrono::Weekday::Sun => Weekday::Sun,
        }
    }
}

/// Why an occurrence is not where the rule would naively have put it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shift {
    /// The local time did not exist; moved forward to the first valid instant.
    Gap,
    /// The local time happened twice; the first was taken.
    Ambiguous,
}

/// One occurrence of a series.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Occurrence {
    /// The instant this occurrence is at, and its identity.
    ///
    /// This is the `original_start` half of `(series_id, original_start)`
    /// (`DESIGN.md` §3.5) — the name an exception uses to reach this occurrence.
    /// It is stable: a later `occ_move` changes where the occurrence is shown,
    /// never what it is called.
    pub start: i64,
    /// Set when a DST transition moved this occurrence, so a view can say so.
    pub shift: Option<Shift>,
}

/// A recurrence rule: the RFC 5545 subset this design uses.
///
/// Storing the rule keeps the log finite for an infinite series. A daily habit
/// is one event, not 365 a year in a log that syncs to a phone (§3.4).
#[derive(Clone, Debug)]
pub struct Rule {
    pub freq: Freq,
    /// Every *n*-th period. One or more; zero would never advance.
    pub interval: u32,
    /// `BYDAY`. Empty means "whatever day the series starts on".
    by_day: u8,
    /// `BYMONTHDAY`. Empty means "the day of the month the series starts on".
    by_month_day: Vec<u8>,
    /// `COUNT` — stop after this many occurrences.
    pub count: Option<u32>,
    /// `UNTIL` — stop at this instant, inclusive.
    pub until: Option<i64>,
}

impl Rule {
    /// A rule with no restrictions beyond its frequency.
    ///
    /// Unbounded by default, which is the common case rather than an edge one:
    /// "get up at 7" has no end date, and §3.4 accepts that explicitly.
    pub fn new(freq: Freq, interval: u32) -> Rule {
        Rule {
            freq,
            interval: interval.max(1),
            by_day: 0,
            by_month_day: Vec::new(),
            count: None,
            until: None,
        }
    }

    /// Restrict to particular weekdays.
    pub fn on_days(mut self, days: &[Weekday]) -> Rule {
        self.by_day = days.iter().fold(0, |acc, d| acc | d.bit());
        self
    }

    /// Restrict to particular days of the month.
    pub fn on_month_days(mut self, days: &[u8]) -> Rule {
        self.by_month_day = days
            .iter()
            .copied()
            .filter(|d| (1..=31).contains(d))
            .collect();
        self
    }

    pub fn count(mut self, n: u32) -> Rule {
        self.count = Some(n);
        self
    }

    pub fn until(mut self, ms: i64) -> Rule {
        self.until = Some(ms);
        self
    }

    fn matches(&self, day: NaiveDate, start: NaiveDate) -> bool {
        if self.by_day != 0 && self.by_day & Weekday::of(day).bit() == 0 {
            return false;
        }
        if !self.by_month_day.is_empty() && !self.by_month_day.contains(&(day.day() as u8)) {
            return false;
        }
        if self.by_day == 0 && self.by_month_day.is_empty() {
            // Nothing said otherwise, so the series keeps the shape of its own
            // start: a weekly series recurs on the weekday it began on.
            match self.freq {
                Freq::Weekly => return day.weekday() == start.weekday(),
                Freq::Monthly | Freq::Yearly => return day.day() == start.day(),
                Freq::Daily => return true,
            }
        }
        true
    }
}

/// The window occurrences are expanded over, given the current instant.
///
/// −1 year to +1 year (§3.4). The backward year is the half that is a record of
/// what happened; the forward year covers any planning horizon. Taking `now` as
/// an argument rather than reading it is the whole reason this crate can be
/// trusted to expand the same way everywhere.
pub fn horizon(now: i64) -> (i64, i64) {
    const YEAR_MS: i64 = 365 * 24 * 60 * 60 * 1000;
    (now - YEAR_MS, now + YEAR_MS)
}

/// Expand a series into the occurrences falling inside a window.
///
/// `series_start` is the first occurrence in local wall time, `zone` the zone
/// the series was written in. The window is half-open in the usual way: an
/// occurrence exactly at `to` is outside it.
///
/// `COUNT` is counted from the beginning of the series rather than from the
/// window, because "the first ten" means the first ten — not the first ten this
/// device happened to look at.
pub fn expand(
    rule: &Rule,
    series_start: NaiveDateTime,
    zone: Zone,
    from: i64,
    to: i64,
) -> Vec<Occurrence> {
    let mut out = Vec::new();
    let (time, start_day) = (series_start.time(), series_start.date());

    let mut day = start_day;
    let mut seen = 0u32;

    // A cap on iteration rather than on results. An unbounded series with a
    // restrictive `BYDAY` can pass over many non-matching days, and a rule that
    // matches nothing at all must still terminate.
    let mut guard = 0u32;
    const MAX_STEPS: u32 = 500_000;

    loop {
        guard += 1;
        if guard > MAX_STEPS {
            break;
        }

        let Some(local) = day.and_hms_opt(time.hour(), time.minute(), time.second()) else {
            break;
        };

        if rule.matches(day, start_day) {
            let (ms, shift) = resolve_local(local, zone);

            if let Some(until) = rule.until
                && ms > until
            {
                break;
            }

            seen += 1;
            if let Some(count) = rule.count
                && seen > count
            {
                break;
            }

            if ms >= to {
                break;
            }
            if ms >= from {
                out.push(Occurrence { start: ms, shift });
            }
        }

        let Some(next) = advance(rule, day, start_day) else {
            break;
        };
        day = next;
    }

    out
}

/// The next candidate day.
///
/// For a restricted rule the step is one day and `matches` does the filtering;
/// stepping by the interval as well would skip the very days `BYDAY` selects.
fn advance(rule: &Rule, day: NaiveDate, start: NaiveDate) -> Option<NaiveDate> {
    let restricted = rule.by_day != 0 || !rule.by_month_day.is_empty();

    match rule.freq {
        Freq::Daily if !restricted => {
            day.checked_add_signed(chrono::Duration::days(rule.interval as i64))
        }
        Freq::Weekly if !restricted => {
            day.checked_add_signed(chrono::Duration::weeks(rule.interval as i64))
        }
        Freq::Monthly if !restricted => add_months(day, rule.interval, start),
        Freq::Yearly if !restricted => add_months(day, rule.interval * 12, start),
        _ => day.checked_add_signed(chrono::Duration::days(1)),
    }
}

/// Add whole months, keeping the day of the month the series started on.
///
/// A series starting on the 31st simply has no occurrence in a month that has
/// no 31st. Clamping to the 30th instead would invent an occurrence on a day
/// the rule never named, and it would then differ from what the same rule
/// produces on a device that clamped differently.
fn add_months(day: NaiveDate, months: u32, start: NaiveDate) -> Option<NaiveDate> {
    let total = day.year() * 12 + (day.month0() as i32) + months as i32;
    let (y, m0) = (total.div_euclid(12), total.rem_euclid(12) as u32);

    // Walk forward until the wanted day-of-month exists, rather than clamping.
    let wanted = start.day();
    let mut y = y;
    let mut m0 = m0;
    for _ in 0..48 {
        if let Some(d) = NaiveDate::from_ymd_opt(y, m0 + 1, wanted) {
            return Some(d);
        }
        let total = y * 12 + m0 as i32 + months.max(1) as i32;
        y = total.div_euclid(12);
        m0 = total.rem_euclid(12) as u32;
    }
    None
}

/// Turn a local wall time into an instant, applying the DST policy.
pub(crate) fn resolve_local(local: NaiveDateTime, zone: Zone) -> (i64, Option<Shift>) {
    match zone.tz().from_local_datetime(&local) {
        LocalResult::Single(t) => (t.timestamp_millis(), None),

        // Fall back: the same local time happens twice. The first wins, and the
        // occurrence records that there was a choice to make.
        LocalResult::Ambiguous(first, _second) => {
            (first.timestamp_millis(), Some(Shift::Ambiguous))
        }

        // Spring forward: this local time does not exist. Walk forward a minute
        // at a time to the first that does — a gap is an hour at most, and
        // searching beats hard-coding an offset that is not the same everywhere.
        LocalResult::None => {
            let mut probe = local;
            for _ in 0..(4 * 60) {
                probe += chrono::Duration::minutes(1);
                if let LocalResult::Single(t) = zone.tz().from_local_datetime(&probe) {
                    return (t.timestamp_millis(), Some(Shift::Gap));
                }
                if let LocalResult::Ambiguous(t, _) = zone.tz().from_local_datetime(&probe) {
                    return (t.timestamp_millis(), Some(Shift::Gap));
                }
            }
            // Unreachable for any real zone; a transition longer than four
            // hours would be a database this code should not silently paper
            // over. Placing it at the naive UTC reading is wrong, so the
            // occurrence is reported as a gap at the local time's UTC value and
            // the marker says it moved.
            (local.and_utc().timestamp_millis(), Some(Shift::Gap))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn denver() -> Zone {
        Zone::new("America/Denver").unwrap()
    }

    fn at(y: i32, m: u32, d: u32, h: u32, min: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(h, min, 0)
            .unwrap()
    }

    fn local_of(ms: i64, zone: Zone) -> NaiveDateTime {
        use chrono::DateTime;
        DateTime::from_timestamp_millis(ms)
            .unwrap()
            .with_timezone(&zone.tz())
            .naive_local()
    }

    #[test]
    fn a_daily_series_keeps_its_local_time_across_a_dst_boundary() {
        // The reason expansion happens in local wall time at all. In UTC these
        // occurrences are not an equal number of hours apart.
        let rule = Rule::new(Freq::Daily, 1);
        let start = at(2026, 3, 6, 7, 0);
        let (from, to) = (
            at(2026, 3, 1, 0, 0).and_utc().timestamp_millis(),
            at(2026, 3, 15, 0, 0).and_utc().timestamp_millis(),
        );

        let occ = expand(&rule, start, denver(), from, to);
        assert!(occ.len() >= 7);
        for o in &occ {
            let l = local_of(o.start, denver());
            assert_eq!(l.hour(), 7, "an occurrence drifted off 07:00 local");
            assert_eq!(l.minute(), 0);
        }
    }

    #[test]
    fn a_nonexistent_local_time_moves_forward_and_says_so() {
        // 02:30 on the day the clocks jump 02:00 to 03:00 is not a time.
        let rule = Rule::new(Freq::Daily, 1);
        let start = at(2026, 3, 8, 2, 30);
        let (from, to) = (
            at(2026, 3, 8, 0, 0).and_utc().timestamp_millis(),
            at(2026, 3, 9, 0, 0).and_utc().timestamp_millis(),
        );

        let occ = expand(&rule, start, denver(), from, to);
        let first = occ.first().expect("the occurrence was dropped entirely");
        assert_eq!(first.shift, Some(Shift::Gap), "the shift was not recorded");

        let l = local_of(first.start, denver());
        assert_eq!(
            l.hour(),
            3,
            "expected the first valid instant after the gap"
        );
    }

    #[test]
    fn an_ambiguous_local_time_takes_the_first_and_says_so() {
        // 01:30 happens twice on the day the clocks fall back.
        let rule = Rule::new(Freq::Daily, 1);
        let start = at(2026, 11, 1, 1, 30);
        let (from, to) = (
            at(2026, 11, 1, 0, 0).and_utc().timestamp_millis(),
            at(2026, 11, 2, 0, 0).and_utc().timestamp_millis(),
        );

        let occ = expand(&rule, start, denver(), from, to);
        let first = occ.first().expect("the occurrence was dropped entirely");
        assert_eq!(first.shift, Some(Shift::Ambiguous));

        // The earlier of the two, which is the one still on daylight time.
        let second = zone_ms(at(2026, 11, 1, 1, 30), denver(), false);
        assert!(first.start < second, "the later of the two was taken");
    }

    fn zone_ms(local: NaiveDateTime, zone: Zone, first: bool) -> i64 {
        match zone.tz().from_local_datetime(&local) {
            LocalResult::Single(t) => t.timestamp_millis(),
            LocalResult::Ambiguous(a, b) => {
                if first {
                    a.timestamp_millis()
                } else {
                    b.timestamp_millis()
                }
            }
            LocalResult::None => 0,
        }
    }

    #[test]
    fn an_ordinary_occurrence_records_no_shift() {
        // The marker has to mean something, so it must not be set routinely.
        let rule = Rule::new(Freq::Daily, 1);
        let start = at(2026, 6, 1, 7, 0);
        let (from, to) = (
            at(2026, 6, 1, 0, 0).and_utc().timestamp_millis(),
            at(2026, 6, 5, 0, 0).and_utc().timestamp_millis(),
        );
        for o in expand(&rule, start, denver(), from, to) {
            assert_eq!(o.shift, None);
        }
    }

    #[test]
    fn weekdays_only_skips_the_weekend() {
        let rule = Rule::new(Freq::Weekly, 1).on_days(&[
            Weekday::Mon,
            Weekday::Tue,
            Weekday::Wed,
            Weekday::Thu,
            Weekday::Fri,
        ]);
        let start = at(2026, 6, 1, 7, 0); // a Monday
        let (from, to) = (
            at(2026, 6, 1, 0, 0).and_utc().timestamp_millis(),
            at(2026, 6, 8, 0, 0).and_utc().timestamp_millis(),
        );

        let occ = expand(&rule, start, denver(), from, to);
        assert_eq!(occ.len(), 5, "a weekday rule produced a weekend");
        for o in &occ {
            let d = local_of(o.start, denver()).date();
            assert!(!matches!(
                d.weekday(),
                chrono::Weekday::Sat | chrono::Weekday::Sun
            ));
        }
    }

    #[test]
    fn an_interval_skips_periods() {
        let rule = Rule::new(Freq::Daily, 3);
        let start = at(2026, 6, 1, 7, 0);
        let (from, to) = (
            at(2026, 6, 1, 0, 0).and_utc().timestamp_millis(),
            at(2026, 6, 10, 0, 0).and_utc().timestamp_millis(),
        );

        let occ = expand(&rule, start, denver(), from, to);
        let days: alloc::vec::Vec<u32> = occ
            .iter()
            .map(|o| local_of(o.start, denver()).date().day())
            .collect();
        assert_eq!(days, alloc::vec![1, 4, 7]);
    }

    #[test]
    fn a_monthly_series_on_the_31st_skips_months_without_one() {
        // Clamping to the 30th would invent an occurrence on a day the rule
        // never named — and a device that clamped differently would disagree.
        let rule = Rule::new(Freq::Monthly, 1);
        let start = at(2026, 1, 31, 9, 0);
        let (from, to) = (
            at(2026, 1, 1, 0, 0).and_utc().timestamp_millis(),
            at(2026, 7, 1, 0, 0).and_utc().timestamp_millis(),
        );

        let occ = expand(&rule, start, denver(), from, to);
        for o in &occ {
            assert_eq!(local_of(o.start, denver()).date().day(), 31);
        }
        // Jan, Mar, May — not February, April or June.
        assert_eq!(occ.len(), 3);
    }

    #[test]
    fn count_is_counted_from_the_series_not_the_window() {
        // "The first ten" means the first ten, not the first ten this device
        // happened to look at.
        let rule = Rule::new(Freq::Daily, 1).count(3);
        let start = at(2026, 6, 1, 7, 0);

        let all = expand(
            &rule,
            start,
            denver(),
            at(2026, 5, 1, 0, 0).and_utc().timestamp_millis(),
            at(2026, 7, 1, 0, 0).and_utc().timestamp_millis(),
        );
        assert_eq!(all.len(), 3);

        // A window starting after the series does not get a fresh three.
        let later = expand(
            &rule,
            start,
            denver(),
            at(2026, 6, 3, 0, 0).and_utc().timestamp_millis(),
            at(2026, 7, 1, 0, 0).and_utc().timestamp_millis(),
        );
        assert_eq!(later.len(), 1, "COUNT restarted inside the window");
    }

    #[test]
    fn until_bounds_the_series_inclusively() {
        let rule = Rule::new(Freq::Daily, 1).until(zone_ms(at(2026, 6, 3, 7, 0), denver(), true));
        let start = at(2026, 6, 1, 7, 0);

        let occ = expand(
            &rule,
            start,
            denver(),
            at(2026, 6, 1, 0, 0).and_utc().timestamp_millis(),
            at(2026, 7, 1, 0, 0).and_utc().timestamp_millis(),
        );
        assert_eq!(occ.len(), 3, "UNTIL should include its own instant");
    }

    #[test]
    fn an_unbounded_series_terminates() {
        // "get up at 7" has no end date (§3.4). The window is what bounds the
        // work, not the rule.
        let rule = Rule::new(Freq::Daily, 1);
        let start = at(2020, 1, 1, 7, 0);
        let (from, to) = horizon(at(2026, 6, 1, 0, 0).and_utc().timestamp_millis());

        let occ = expand(&rule, start, denver(), from, to);
        assert!(
            occ.len() > 700 && occ.len() < 740,
            "{} occurrences",
            occ.len()
        );
    }

    #[test]
    fn the_window_is_half_open() {
        let rule = Rule::new(Freq::Daily, 1);
        let start = at(2026, 6, 1, 7, 0);
        let first = zone_ms(at(2026, 6, 1, 7, 0), denver(), true);

        let inclusive = expand(&rule, start, denver(), first, first + 1);
        assert_eq!(
            inclusive.len(),
            1,
            "an occurrence exactly at `from` is inside"
        );

        let exclusive = expand(&rule, start, denver(), first + 1, first + 2);
        assert!(exclusive.is_empty(), "an occurrence at `to` leaked in");
    }

    #[test]
    fn expansion_is_a_pure_function_of_its_inputs() {
        // The property the whole crate exists to make true: the same rule
        // expands identically every time, so it expands identically everywhere.
        let rule = Rule::new(Freq::Weekly, 1).on_days(&[Weekday::Mon, Weekday::Thu]);
        let start = at(2026, 1, 5, 8, 15);
        let (from, to) = horizon(at(2026, 6, 1, 0, 0).and_utc().timestamp_millis());

        let a = expand(&rule, start, denver(), from, to);
        let b = expand(&rule, start, denver(), from, to);
        assert_eq!(a, b);
    }

    #[test]
    fn a_zone_changes_the_instants_but_not_the_local_times() {
        let rule = Rule::new(Freq::Daily, 1);
        let start = at(2026, 6, 1, 7, 0);
        let (from, to) = (
            at(2026, 6, 1, 0, 0).and_utc().timestamp_millis(),
            at(2026, 6, 4, 0, 0).and_utc().timestamp_millis(),
        );

        let tokyo = Zone::new("Asia/Tokyo").unwrap();
        let a = expand(&rule, start, denver(), from, to);
        let b = expand(&rule, start, tokyo, from, to);

        assert_ne!(
            a[0].start, b[0].start,
            "two zones produced the same instant"
        );
        assert_eq!(local_of(a[0].start, denver()).hour(), 7);
        assert_eq!(local_of(b[0].start, tokyo).hour(), 7);
    }
}
