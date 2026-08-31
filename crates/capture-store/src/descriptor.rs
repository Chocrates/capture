//! The one-line rendering of what a note currently *is*.
//!
//! §3.1.4 asks for three things on a promoted note: the captured text, the
//! current fields, and a descriptor — a human rendering of the fields,
//! regenerated whenever they change. The point of the third is that drift
//! between the first two stays visible: the text says "every day at 7", the
//! fields say weekdays, and a reader can see both without either being
//! rewritten to agree with the other.
//!
//! # Rendered when written, not when read
//!
//! Stored beside the fields rather than computed per view. A calendar drawing a
//! week should not be formatting dates for every row it draws, and a descriptor
//! that differed between two surfaces because they rendered it differently
//! would be a third version of the truth.
//!
//! # It says what is there and nothing else
//!
//! No "no date set", no "untitled", no invented defaults. A field that is
//! absent simply does not appear — an absent end time is not the same claim as
//! a zero-length event, and §3.1.3's whole argument is that annotations must
//! not add facts the capture did not contain.

use capture_core::body::{Promoted, When};
use capture_time::Zone;

/// A short human rendering of a promotion's fields.
pub fn render(p: &Promoted) -> String {
    match p {
        Promoted::Note => String::new(),

        Promoted::Task(t) => {
            let mut s = String::from(if t.done { "done" } else { "task" });
            if let Some(due) = &t.due {
                s.push_str(" · due ");
                s.push_str(due);
            }
            s
        }

        Promoted::Event(e) => {
            let mut s = when(&e.when);
            if let Some(l) = &e.location {
                s.push_str(" · ");
                s.push_str(l);
            }
            s
        }

        Promoted::Series(sf) => {
            let mut s = rule(&sf.rule);
            if let When::Timed { start_ms, tz, .. } = &sf.when {
                // The clock the series happens at, read in its own zone. Taking
                // the remainder of the instant instead would be the local time
                // only in UTC, and silently an hour or several out everywhere
                // else — which is the whole reason the zone is stored.
                if let Some(z) = Zone::new(tz) {
                    let (h, m) = capture_time::civil_clock(*start_ms, z);
                    s.push_str(&format!(" at {h:02}:{m:02}"));
                }
            }
            if sf.carries_forward {
                // Worth saying, because it is the difference between a missed
                // walk and a missed dose (§3.6).
                s.push_str(" · carries forward");
            }
            s
        }
    }
}

/// A date, or a moment rendered in the zone it was written in.
fn when(w: &When) -> String {
    match w {
        When::AllDay {
            start_date,
            end_date,
        } => {
            if start_date == end_date {
                start_date.clone()
            } else {
                format!("{start_date} – {end_date}")
            }
        }
        When::Timed {
            start_ms,
            end_ms,
            tz,
        } => {
            // Rendered in the capture's zone rather than the reader's. A note
            // taken at 23:40 in one zone must not read as a different day
            // somewhere else — the same reason the zone is stored at all.
            let zone = Zone::new(tz);
            let start = zone
                .map(|z| stamp(*start_ms, z))
                .unwrap_or_else(|| format!("{start_ms}"));
            match (end_ms, zone) {
                (Some(e), Some(z)) => format!("{start}–{}", clock(*e, z)),
                _ => start,
            }
        }
    }
}

fn stamp(ms: i64, zone: Zone) -> String {
    let d = capture_time::civil_date(ms, zone);
    format!("{d} {}", clock(ms, zone))
}

fn clock(ms: i64, zone: Zone) -> String {
    let (h, m) = capture_time::civil_clock(ms, zone);
    format!("{h:02}:{m:02}")
}

/// A recurrence rule, in words.
fn rule(r: &capture_core::body::RuleSpec) -> String {
    let unit = match r.freq {
        0 => "day",
        1 => "week",
        2 => "month",
        _ => "year",
    };

    let mut s = if r.interval == 1 {
        format!("every {unit}")
    } else {
        format!("every {} {unit}s", r.interval)
    };

    if r.by_day != 0 {
        let names = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        let days: Vec<&str> = names
            .iter()
            .enumerate()
            .filter(|(i, _)| r.by_day & (1 << i) != 0)
            .map(|(_, n)| *n)
            .collect();

        // The two sets people actually mean get their own words. "Mon, Tue,
        // Wed, Thu, Fri" is what the bitmask says and not what anybody wrote.
        s = match r.by_day {
            0b0001_1111 => String::from("every weekday"),
            0b0110_0000 => String::from("every weekend"),
            _ => format!("every {}", days.join(", ")),
        };
        if r.interval != 1 {
            s = format!("{s}, every {} weeks", r.interval);
        }
    }

    if let Some(n) = r.count {
        s.push_str(&format!(" · {n} times"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use capture_core::body::{EventFields, RuleSpec, SeriesFields, TaskFields};

    fn spec(freq: u8, interval: u32, by_day: u8) -> RuleSpec {
        RuleSpec {
            freq,
            interval,
            by_day,
            by_month_day: Vec::new(),
            count: None,
            until: None,
        }
    }

    #[test]
    fn a_task_says_whether_it_is_done_and_when_it_is_due() {
        assert_eq!(
            render(&Promoted::Task(TaskFields {
                done: false,
                due: Some("2026-08-14".into()),
                order: None
            })),
            "task · due 2026-08-14"
        );
        assert_eq!(
            render(&Promoted::Task(TaskFields {
                done: true,
                due: None,
                order: None
            })),
            "done"
        );
    }

    #[test]
    fn an_absent_field_simply_does_not_appear() {
        // No "no date set". An absent end time is not a zero-length event, and
        // a descriptor must not add facts the capture did not contain.
        let e = Promoted::Event(EventFields {
            when: When::Timed {
                start_ms: 1_780_326_000_000,
                end_ms: None,
                tz: "America/Denver".into(),
            },
            location: None,
            attendees: Vec::new(),
        });
        let s = render(&e);
        assert!(!s.contains('–'), "an end time appeared from nowhere: {s}");
        assert!(!s.contains("no "), "{s}");
    }

    #[test]
    fn a_single_day_is_not_rendered_as_a_range() {
        assert_eq!(
            render(&Promoted::Event(EventFields {
                when: When::AllDay {
                    start_date: "2026-08-14".into(),
                    end_date: "2026-08-14".into()
                },
                location: None,
                attendees: Vec::new(),
            })),
            "2026-08-14"
        );
    }

    #[test]
    fn a_range_shows_both_ends() {
        assert_eq!(
            render(&Promoted::Event(EventFields {
                when: When::AllDay {
                    start_date: "2026-08-01".into(),
                    end_date: "2026-08-14".into()
                },
                location: None,
                attendees: Vec::new(),
            })),
            "2026-08-01 – 2026-08-14"
        );
    }

    #[test]
    fn a_moment_renders_in_the_zone_it_was_written_in() {
        // Not the reader's zone. A note taken at 23:40 in one zone must not
        // read as a different day somewhere else.
        let at = Promoted::Event(EventFields {
            when: When::Timed {
                start_ms: 1_780_326_000_000,
                end_ms: None,
                tz: "America/Denver".into(),
            },
            location: None,
            attendees: Vec::new(),
        });
        let denver = render(&at);

        let elsewhere = Promoted::Event(EventFields {
            when: When::Timed {
                start_ms: 1_780_326_000_000,
                end_ms: None,
                tz: "Asia/Tokyo".into(),
            },
            location: None,
            attendees: Vec::new(),
        });
        assert_ne!(denver, render(&elsewhere));
        assert!(denver.starts_with("2026-06-01 09:00"), "{denver}");
    }

    #[test]
    fn the_sets_people_mean_get_the_words_people_use() {
        // "Mon, Tue, Wed, Thu, Fri" is what the bitmask says, not what anyone
        // wrote down.
        let weekdays = Promoted::Series(SeriesFields {
            when: When::AllDay {
                start_date: "2026-06-01".into(),
                end_date: "2026-06-01".into(),
            },
            rule: spec(1, 1, 0b0001_1111),
            carries_forward: false,
            split_from: None,
        });
        assert_eq!(render(&weekdays), "every weekday");

        let weekend = Promoted::Series(SeriesFields {
            when: When::AllDay {
                start_date: "2026-06-01".into(),
                end_date: "2026-06-01".into(),
            },
            rule: spec(1, 1, 0b0110_0000),
            carries_forward: false,
            split_from: None,
        });
        assert_eq!(render(&weekend), "every weekend");
    }

    #[test]
    fn carrying_forward_is_said_out_loud() {
        // The difference between a missed walk and a missed dose.
        let s = Promoted::Series(SeriesFields {
            when: When::AllDay {
                start_date: "2026-06-01".into(),
                end_date: "2026-06-01".into(),
            },
            rule: spec(0, 1, 0),
            carries_forward: true,
            split_from: None,
        });
        assert!(render(&s).contains("carries forward"), "{}", render(&s));
    }

    #[test]
    fn a_series_clock_is_read_in_its_own_zone() {
        // The bug this replaced took the remainder of the instant, which is the
        // local time only in UTC. A series at 07:00 in Denver would have read
        // as 13:00 or 14:00 depending on the season.
        let seven_denver = 1_780_318_800_000; // 2026-06-01 07:00 America/Denver
        let s = Promoted::Series(SeriesFields {
            when: When::Timed {
                start_ms: seven_denver,
                end_ms: None,
                tz: "America/Denver".into(),
            },
            rule: spec(0, 1, 0),
            carries_forward: false,
            split_from: None,
        });
        assert!(render(&s).ends_with(" at 07:00"), "{}", render(&s));
    }

    #[test]
    fn an_interval_reads_as_an_interval() {
        let s = Promoted::Series(SeriesFields {
            when: When::AllDay {
                start_date: "2026-06-01".into(),
                end_date: "2026-06-01".into(),
            },
            rule: spec(1, 2, 0),
            carries_forward: false,
            split_from: None,
        });
        assert_eq!(render(&s), "every 2 weeks");
    }
}
