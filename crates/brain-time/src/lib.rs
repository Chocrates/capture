//! Time, for a log that must fold identically on every device that reads it.
//!
//! # Two kinds of time, and they are not interchangeable
//!
//! Most calendar bugs come from storing one where the other belongs
//! (`DESIGN.md` §3.2).
//!
//! An [`Instant`] is a moment: UTC milliseconds *plus* the IANA zone it was
//! created in. Both are required. UTC alone cannot expand a recurrence
//! correctly, because "07:00 daily" means 07:00 local on both sides of a DST
//! boundary and that is not a fixed offset from UTC.
//!
//! A [`FloatingDate`] is a date: a vacation, a birthday, a deadline. It has no
//! zone, no time, and no conversion — 1–14 August is 1–14 August in Tokyo and
//! in Denver. Converting all-day events to UTC instants is the single most
//! common calendar bug in the wild, and it makes events silently jump a day for
//! anyone east or west of the machine that stored them.
//!
//! **There is deliberately no way to turn a `FloatingDate` into an `Instant`.**
//! Not a discouraged method, not a documented footgun — no such function
//! exists, in either direction. A caller that genuinely needs an instant for a
//! date has to say which zone and which time of day it means, at which point it
//! is making the decision explicitly rather than inheriting one.
//!
//! # Nothing here can read a clock
//!
//! `no_std`, and both dependencies are taken without the features that reach a
//! clock. Every entry point that needs the current time takes it as a value.
//!
//! This is tedious and it is the point. Recurrence is the one place in this
//! system where a wrong answer is invisible: an occurrence at the wrong instant
//! looks exactly like an occurrence at the right one. The only thing that
//! reliably catches that is expansion being a pure function of its inputs, so
//! the same rule expands the same way on every device — which is a property a
//! test can assert, and a convention is not.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

mod rule;

pub use rule::{Freq, Occurrence, Rule, Shift, Weekday, expand, horizon};

use chrono::NaiveDate;
use chrono_tz::Tz;

/// A named IANA timezone.
///
/// Held as the parsed zone rather than the string so that a zone which does not
/// exist cannot be constructed and then fail later, during expansion, where the
/// failure would be a missing occurrence rather than an error.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Zone(Tz);

impl Zone {
    /// Look up a zone by its IANA identifier.
    ///
    /// Returns `None` for a name the embedded database does not know. Callers
    /// hold onto the answer: an event carries the zone id it was written with,
    /// and a database that no longer lists it is a real condition rather than
    /// something to paper over with UTC.
    pub fn new(name: &str) -> Option<Zone> {
        name.parse::<Tz>().ok().map(Zone)
    }

    /// The identifier, as it would be written into an event.
    pub fn name(&self) -> &'static str {
        self.0.name()
    }

    pub(crate) fn tz(&self) -> Tz {
        self.0
    }
}

/// A moment, and the zone it was written in.
///
/// The zone is not decoration. It is what makes the recurrence containing this
/// instant expandable, and what lets a view show the time the capture actually
/// happened at rather than the reader's local rendering of it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Instant {
    /// Milliseconds since the Unix epoch, UTC.
    pub ms: i64,
    /// The zone this was created in.
    pub zone: Zone,
}

impl Instant {
    pub fn new(ms: i64, zone: Zone) -> Instant {
        Instant { ms, zone }
    }
}

/// A date with no zone and no time of day.
///
/// Stored and compared exactly as written. There is no conversion to an
/// [`Instant`] anywhere in this crate; see the module documentation for why.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct FloatingDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl FloatingDate {
    /// A date, if it is one.
    ///
    /// Rejects the 31st of February rather than rolling it forward. A date that
    /// does not exist is a mistake upstream, and silently moving it would put
    /// a deadline on a day nobody chose.
    pub fn new(year: i32, month: u8, day: u8) -> Option<FloatingDate> {
        NaiveDate::from_ymd_opt(year, month as u32, day as u32)?;
        Some(FloatingDate { year, month, day })
    }

    /// `YYYY-MM-DD`, which is how it is stored.
    pub fn parse(s: &str) -> Option<FloatingDate> {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        FloatingDate::new(
            s[0..4].parse().ok()?,
            s[5..7].parse().ok()?,
            s[8..10].parse().ok()?,
        )
    }
}

impl core::fmt::Display for FloatingDate {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn a_zone_that_does_not_exist_is_refused() {
        // Refused here rather than at expansion, where the failure would be a
        // missing occurrence instead of an error.
        assert!(Zone::new("America/Denver").is_some());
        assert!(Zone::new("Mars/Olympus_Mons").is_none());
    }

    #[test]
    fn a_zone_keeps_the_name_it_was_written_with() {
        // An event carries the id it was authored with, and a view has to be
        // able to show it back.
        assert_eq!(Zone::new("Europe/London").unwrap().name(), "Europe/London");
    }

    #[test]
    fn a_date_that_does_not_exist_is_refused_rather_than_rolled() {
        // Rolling the 31st of February forward would put a deadline on a day
        // nobody chose, which is worse than refusing it.
        assert!(FloatingDate::new(2026, 2, 29).is_none());
        assert!(
            FloatingDate::new(2024, 2, 29).is_some(),
            "2024 is a leap year"
        );
        assert!(FloatingDate::new(2026, 13, 1).is_none());
        assert!(FloatingDate::new(2026, 4, 31).is_none());
    }

    #[test]
    fn a_date_round_trips_through_its_stored_form() {
        let d = FloatingDate::parse("2026-08-14").unwrap();
        assert_eq!(d, FloatingDate::new(2026, 8, 14).unwrap());
        assert_eq!(d.to_string(), "2026-08-14");
    }

    #[test]
    fn a_malformed_date_is_not_guessed_at() {
        for s in [
            "2026-8-14",
            "26-08-14",
            "2026/08/14",
            "",
            "2026-08-14T00:00",
        ] {
            assert!(FloatingDate::parse(s).is_none(), "{s:?} was accepted");
        }
    }

    #[test]
    fn dates_order_by_calendar_order() {
        let a = FloatingDate::parse("2026-08-01").unwrap();
        let b = FloatingDate::parse("2026-08-14").unwrap();
        let c = FloatingDate::parse("2026-09-01").unwrap();
        assert!(a < b && b < c);
    }
}
