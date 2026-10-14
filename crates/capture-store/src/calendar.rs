//! Laying a day out, so overlapping things sit side by side (§4.4).
//!
//! # Why this is here rather than in a window
//!
//! It was in the desktop's IPC layer, which was fine while the desktop was the
//! only thing that drew a day. A phone draws the same day from the same
//! archive, and a second copy of this would be a second answer to "do these
//! two meetings overlap" — both plausible, neither exercised against the
//! other, and the divergence visible only as two devices disagreeing about
//! what Tuesday looks like.
//!
//! §4.4 asks for column packing and does not say how. This is the ordinary
//! algorithm: occurrences are grouped into clusters that transitively overlap,
//! and within a cluster each takes the first column free at its start. The
//! cluster decides the width, so a day with one three-way overlap at noon does
//! not squeeze the morning into thirds.

/// Something that might occupy space on an hour axis.
///
/// A trait rather than a struct, so a surface packs its own rows in place. The
/// alternative is converting into a shape this module owns and back again, and
/// the conversion is then the thing that can be wrong — in two places, in
/// different ways.
pub trait Timed {
    fn start_ms(&self) -> i64;

    /// Where it ends, if that was written down.
    fn end_ms(&self) -> Option<i64>;

    /// Whether it belongs on the hour axis at all.
    ///
    /// All-day entries and due tasks do not: they have no position on it,
    /// which is the whole reason §3.2 and §4.4 keep them in bands of their
    /// own. Packing them would place them against hours they were never given.
    fn on_axis(&self) -> bool;

    /// Take a column, and the width of the cluster it belongs to.
    fn place(&mut self, column: usize, columns: usize);
}

/// How long an occurrence with no written end occupies, for overlap only.
///
/// Not a duration and never drawn as one. Two things at the same time are
/// still two things at the same time, and a zero-width interval overlaps
/// nothing — so packing would put them in one column and a surface would draw
/// them on top of each other. Fifteen minutes is enough to make them collide
/// and short enough that it cannot manufacture a collision between meetings an
/// hour apart.
pub const UNWRITTEN: i64 = 15 * 60 * 1000;

/// Assign each timed row a column and a cluster width.
pub fn pack<T: Timed>(rows: &mut [T]) {
    let mut timed: Vec<usize> = (0..rows.len()).filter(|&i| rows[i].on_axis()).collect();

    timed.sort_by_key(|&i| (rows[i].start_ms(), -end_of(&rows[i])));

    let mut cluster: Vec<usize> = Vec::new();
    let mut cluster_end = i64::MIN;
    // The end of the last occurrence in each open column.
    let mut columns: Vec<i64> = Vec::new();
    // Columns are assigned as the sweep goes and the width is only known when
    // the cluster closes, so the assignment is remembered and applied after.
    let mut placed: Vec<(usize, usize)> = Vec::new();

    for &i in &timed {
        // A gap closes the cluster: nothing after this point can overlap
        // anything in it, so the width is settled and can be written down.
        if rows[i].start_ms() >= cluster_end && !cluster.is_empty() {
            let width = columns.len().max(1);
            for &j in &cluster {
                let col = placed
                    .iter()
                    .find(|(k, _)| *k == j)
                    .map(|(_, c)| *c)
                    .unwrap_or(0);
                rows[j].place(col, width);
            }
            cluster.clear();
            columns.clear();
            placed.clear();
            cluster_end = i64::MIN;
        }

        let col = match columns.iter().position(|&end| end <= rows[i].start_ms()) {
            Some(c) => c,
            None => {
                columns.push(i64::MIN);
                columns.len() - 1
            }
        };

        let end = end_of(&rows[i]);
        columns[col] = end;
        placed.push((i, col));
        cluster.push(i);
        cluster_end = cluster_end.max(end);
    }

    let width = columns.len().max(1);
    for &j in &cluster {
        let col = placed
            .iter()
            .find(|(k, _)| *k == j)
            .map(|(_, c)| *c)
            .unwrap_or(0);
        rows[j].place(col, width);
    }
}

fn end_of<T: Timed>(o: &T) -> i64 {
    o.end_ms().unwrap_or(o.start_ms() + UNWRITTEN)
}

// ---------------------------------------------------------------- the read ---

/// Where something sits on a day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// A date with no time: an all-day event, or a series with no hour.
    AllDay,
    /// Something at a moment, on the hour axis.
    Timed,
    /// A task due that day.
    Due,
}

/// One thing on one day of the calendar.
#[derive(Clone, Debug)]
pub struct Item {
    pub tgt: capture_core::EventId,
    pub slug: String,
    pub text: Option<String>,
    pub kind: ItemKind,
    /// An instance of a series, so "this and later" means something.
    pub series: bool,
    /// The occurrence's name — where the rule put it — for exceptions.
    /// `None` for a task or an all-day one-off, which have no occurrences.
    pub occ: Option<i64>,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    /// Where the rule put it, when it has been moved somewhere else (§4.4).
    pub moved_from: Option<i64>,
    /// Skipped. Returned rather than left out, so Restore has a place.
    pub skipped: bool,
    pub done: bool,
    /// A title for this occurrence only.
    pub title: Option<String>,
    /// `gap` or `ambiguous`, when a DST transition moved it.
    pub shift: Option<String>,
    /// An hour written without am or pm, which is why this has no time.
    pub unclear_hour: Option<u32>,
    /// The two days the text could mean, of which this is one. Shown on both,
    /// since it is on neither until somebody says (grammar 9).
    pub unclear_day: Option<[capture_time::FloatingDate; 2]>,
    pub column: usize,
    pub columns: usize,
}

impl Timed for Item {
    fn start_ms(&self) -> i64 {
        self.start_ms
    }
    fn end_ms(&self) -> Option<i64> {
        self.end_ms
    }
    // A skipped occurrence is drawn as skipped in the list, never on the axis
    // where it would push something real into a narrower column.
    fn on_axis(&self) -> bool {
        self.kind == ItemKind::Timed && !self.skipped
    }
    fn place(&mut self, column: usize, columns: usize) {
        self.column = column;
        self.columns = columns;
    }
}

/// One local day and what happens on it.
#[derive(Clone, Debug)]
pub struct Day {
    pub date: capture_time::FloatingDate,
    /// All-day first, then timed by start, then what is due.
    pub items: Vec<Item>,
}

impl Day {
    /// Whether a grid should mark this day. Skipped things do not count:
    /// a day whose only appointment was called off is a free day.
    pub fn busy(&self) -> bool {
        self.items.iter().any(|i| !i.skipped)
    }
}

/// The most days one read covers: six weeks, a month grid's worth.
pub const MAX_DAYS: usize = 42;

/// Every day from `from` to `to` inclusive, in `zone`, and what happens on it.
///
/// The one answer to "what is on this day", for every surface. The phone's
/// agenda, the desktop's and the command line each had their own, and the
/// phone's once left out every one-off event; three readings of one question
/// is how a calendar comes to disagree with itself.
///
/// Days are the reader's local days, bounded by local midnights rather than by
/// adding 24 hours, so the day a clock changes is 23 or 25 hours long and
/// nothing slips across its edge.
///
/// Refused beyond the occurrence horizon (§3.4, a year either side of `now`):
/// occurrences there have not been worked out, and an empty month that is
/// really an unexpanded one would read as a free month.
pub fn days(
    store: &crate::projection::Store,
    from: capture_time::FloatingDate,
    to: capture_time::FloatingDate,
    zone: capture_time::Zone,
    now: i64,
) -> crate::Result<Vec<Day>> {
    use crate::Error;
    if to < from {
        return Err(Error::Usage(format!("{to} is before {from}")));
    }
    let midnight = |d: capture_time::FloatingDate| {
        capture_time::at_local(d, 0, 0, zone)
            .ok_or_else(|| Error::Usage(format!("{d} is not a day in this zone")))
    };
    let next = |d: capture_time::FloatingDate| -> crate::Result<capture_time::FloatingDate> {
        let noon = capture_time::at_local(d, 12, 0, zone)
            .ok_or_else(|| Error::Usage(format!("{d} is not a day in this zone")))?;
        capture_time::add_days(noon, zone, 1)
            .ok_or_else(|| Error::Usage(format!("no day after {d}")))
    };

    let (earliest, latest) = capture_time::horizon(now);
    if midnight(from)? < earliest || midnight(next(to)?)? > latest {
        return Err(Error::Usage(String::from(
            "occurrences are only worked out a year either side of today, \
             so a calendar further out than that would show days as free that are not",
        )));
    }

    let mut out = Vec::new();
    let mut d = from;
    loop {
        if out.len() == MAX_DAYS {
            return Err(Error::Usage(format!(
                "a calendar read covers at most {MAX_DAYS} days"
            )));
        }
        let (start, end) = (midnight(d)?, midnight(next(d)?)?);
        out.push(Day {
            date: d,
            items: day(store, d, start, end)?,
        });
        if d == to {
            break;
        }
        d = next(d)?;
    }
    Ok(out)
}

/// One day's items, laid out.
fn day(
    store: &crate::projection::Store,
    date: capture_time::FloatingDate,
    start: i64,
    end: i64,
) -> crate::Result<Vec<Item>> {
    let iso = date.to_string();
    let mut items = Vec::new();

    let item = |tgt: capture_core::EventId, kind: ItemKind, start_ms: i64| -> crate::Result<Item> {
        let text = store.text_of(&tgt)?;
        let promotion = store.promotion_of(&tgt)?;
        Ok(Item {
            tgt,
            slug: store.slug_of(&tgt)?.unwrap_or_default(),
            unclear_hour: crate::promote::asks_hour(text.as_deref(), promotion.as_ref()),
            unclear_day: None,
            series: promotion.as_ref().is_some_and(|p| p.kind == "series"),
            text,
            kind,
            occ: None,
            start_ms,
            end_ms: None,
            moved_from: None,
            skipped: false,
            done: false,
            title: None,
            shift: None,
            column: 0,
            columns: 1,
        })
    };

    for tgt in store.all_day_on(&iso)? {
        items.push(item(tgt, ItemKind::AllDay, start)?);
    }

    for (tgt, o) in store.happening_between(start, end)? {
        let kind = if o.all_day {
            ItemKind::AllDay
        } else {
            ItemKind::Timed
        };
        let mut i = item(tgt, kind, o.start_ms)?;
        i.occ = Some(o.occ);
        i.end_ms = o.end_ms;
        i.moved_from = (o.start_ms != o.occ).then_some(o.occ);
        i.skipped = o.cancelled;
        i.done = o.done;
        i.title = o.title;
        i.shift = o.shift;
        items.push(i);
    }

    for (tgt, done) in store.tasks_due_on(&iso)? {
        let mut i = item(tgt, ItemKind::Due, start)?;
        i.done = done;
        items.push(i);
    }

    // Asked on both days, with the all-day things: it has no time until it has
    // a day, and it cannot be found if it is on neither.
    for (tgt, [first, second]) in store.unclear_days_on(&iso)? {
        let mut i = item(tgt, ItemKind::AllDay, start)?;
        i.unclear_day = capture_time::FloatingDate::parse(&first)
            .zip(capture_time::FloatingDate::parse(&second))
            .map(|(a, b)| [a, b]);
        items.push(i);
    }

    pack(&mut items);
    items.sort_by_key(|i| {
        let band = match i.kind {
            ItemKind::AllDay => 0,
            ItemKind::Timed => 1,
            ItemKind::Due => 2,
        };
        (band, i.start_ms)
    });
    Ok(items)
}

// ------------------------------------------------- one occurrence, by people

/// The occurrence of a series on a local date, by the name exceptions use.
///
/// People name an occurrence by its day — "this Monday's gym" — and the log
/// names it by the instant the rule produced (§3.5). This is the translation,
/// and it refuses rather than guesses: a day the series does not happen on is
/// answered with the nearest days it does, so the next try is not a guess too.
pub fn occurrence_on(
    store: &crate::projection::Store,
    tgt: &capture_core::EventId,
    date: capture_time::FloatingDate,
    zone: capture_time::Zone,
) -> crate::Result<i64> {
    use crate::Error;
    let occs: Vec<i64> = store.occurrences_of(tgt)?.iter().map(|o| o.occ).collect();
    if occs.is_empty() {
        return Err(Error::Usage(String::from(
            "that is not a repeating series, so it has no occurrences to name",
        )));
    }
    let on = |occ: &i64| capture_time::civil_date(*occ, zone);
    let hits: Vec<i64> = occs.iter().copied().filter(|o| on(o) == date).collect();
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => {
            let before = occs.iter().filter(|o| on(o) < date).max().map(on);
            let after = occs.iter().filter(|o| on(o) > date).min().map(on);
            let nearest: Vec<String> = [before, after]
                .into_iter()
                .flatten()
                .map(|d| d.to_string())
                .collect();
            Err(Error::Usage(format!(
                "it does not happen on {date}; the nearest days it does are {}",
                nearest.join(" and ")
            )))
        }
        many => Err(Error::Usage(format!(
            "it happens {} times on {date}; name one by its start in milliseconds: {}",
            many.len(),
            many.iter()
                .map(|m| m.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// Where "thursday 10am" puts an occurrence, without writing anything.
///
/// Read by the same recognizer as a capture. "thursday" counts from the
/// occurrence's own day when that is still ahead, and from now when it has
/// passed: next week's gym moved "to thursday" is next week's Thursday — the
/// first version read it against today and put it four days *before* the
/// session it was moving — and a gym missed last Monday moved "to thursday"
/// is the coming one, not a Thursday already gone.
///
/// - A day and a time is that moment.
/// - A day alone keeps the occurrence's time of day: "move it to Thursday"
///   means the same slot on another day.
/// - A bare hour is refused with the question a capture would ask, rather
///   than guessed (D1 of this change).
///
/// The length is kept, so an hour-long session moved is still an hour long.
/// Returned as `(start, end)` so a surface can show it before confirming.
pub fn resolve_move(
    store: &crate::projection::Store,
    tgt: &capture_core::EventId,
    occ: i64,
    text: &str,
    now: i64,
    zone: capture_time::Zone,
) -> crate::Result<(i64, Option<i64>)> {
    use crate::Error;
    use capture_parse::{Shape, When};

    let row = store
        .occurrences_of(tgt)?
        .into_iter()
        .find(|o| o.occ == occ)
        .ok_or_else(|| Error::Usage(String::from("there is no such occurrence")))?;

    let its_day =
        capture_time::at_local(capture_time::civil_date(occ, zone), 0, 0, zone).unwrap_or(occ);
    let r = capture_parse::recognize(text, now.max(its_day), zone);
    if let Some(u) = r.unclear_hour {
        let h = u.hour;
        return Err(Error::Usage(format!(
            "\"{h}\" could be {h}am or {h}pm — say which"
        )));
    }
    if let Some(u) = r.unclear_day {
        return Err(Error::Usage(format!(
            "\"{}\" could be {} or {} — say which",
            &text[u.start..u.end],
            day_label(u.choices[0]),
            day_label(u.choices[1])
        )));
    }
    let start = match r.shape {
        Shape::Point(When::Moment(ms)) => ms,
        Shape::Point(When::Date(d)) | Shape::Task { due: Some(d) } => {
            let (h, m) = if row.all_day {
                (0, 0)
            } else {
                capture_time::civil_clock(row.start_ms, zone)
            };
            capture_time::at_local(d, h, m, zone)
                .ok_or_else(|| Error::Usage(format!("{d} is not a day in this zone")))?
        }
        _ => {
            return Err(Error::Usage(format!(
                "could not read a day or a time in {text:?} — try \"thursday 10am\""
            )));
        }
    };

    // How long it lasts: its own end if an exception wrote one, else the
    // series' length.
    let length = row.end_ms.map(|e| e - row.start_ms).or_else(|| {
        match store.promotion_of(tgt).ok().flatten()?.as_ {
            capture_core::body::Promoted::Series(f) => match f.when {
                capture_core::body::When::Timed {
                    start_ms,
                    end_ms: Some(e),
                    ..
                } => Some(e - start_ms),
                _ => None,
            },
            _ => None,
        }
    });
    Ok((start, length.map(|l| start + l)))
}

// ------------------------------------------------------------- grid shapes

/// The two shapes a calendar is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Month,
    Week,
}

/// Which days a calendar draws around a chosen day, and where its arrows go.
///
/// Here rather than on each surface: which weekday the 1st falls on and how
/// many days October has are date arithmetic, and the phone may not do any
/// (its build refuses `java.time`) — a second answer to "what day is this"
/// is a calendar that disagrees with itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// The first and last days drawn: whole Monday-to-Sunday weeks.
    pub from: capture_time::FloatingDate,
    pub to: capture_time::FloatingDate,
    /// The month's own first and last day, or the week's; days outside are
    /// drawn dimmed in a month grid.
    pub first: capture_time::FloatingDate,
    pub last: capture_time::FloatingDate,
    /// "October 2026", or "12 – 18 Oct 2026".
    pub title: String,
    /// The same day a month or a week either side, clamped to the month's
    /// length: from the 31st of October, a month on is the 30th of November.
    pub previous: capture_time::FloatingDate,
    pub next: capture_time::FloatingDate,
}

pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
pub const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// Monday is zero, as everywhere else in this archive.
pub fn weekday(d: capture_time::FloatingDate) -> u32 {
    // 1970-01-01 was a Thursday. Counted at noon UTC so no offset can carry
    // the count across midnight.
    let utc = capture_time::Zone::new("UTC").expect("UTC is always present");
    let ms = capture_time::at_local(d, 12, 0, utc).unwrap_or(0);
    ((ms.div_euclid(86_400_000) + 3).rem_euclid(7)) as u32
}

/// A date some whole number of days away.
pub fn shift(d: capture_time::FloatingDate, days: i64) -> Option<capture_time::FloatingDate> {
    let utc = capture_time::Zone::new("UTC")?;
    capture_time::add_days(capture_time::at_local(d, 12, 0, utc)?, utc, days)
}

fn month_length(year: i32, month: u8) -> u8 {
    (28..=31)
        .rev()
        .find(|&n| capture_time::FloatingDate::new(year, month, n).is_some())
        .unwrap_or(28)
}

/// The same day of the month `n` months away, clamped to that month's length.
fn months_away(d: capture_time::FloatingDate, n: i32) -> Option<capture_time::FloatingDate> {
    let index = d.year * 12 + (d.month as i32 - 1) + n;
    let (year, month) = (index.div_euclid(12), (index.rem_euclid(12) + 1) as u8);
    capture_time::FloatingDate::new(year, month, d.day.min(month_length(year, month)))
}

/// "Mon 12 Oct".
pub fn day_label(d: capture_time::FloatingDate) -> String {
    format!(
        "{} {} {}",
        DAYS[weekday(d) as usize],
        d.day,
        &MONTHS[d.month as usize - 1][..3]
    )
}

/// The days a calendar of `shape` draws around `day`.
pub fn span(
    day: capture_time::FloatingDate,
    shape: Shape,
    _zone: capture_time::Zone,
) -> crate::Result<Span> {
    let bad = || crate::Error::Usage(format!("{day} has no calendar around it"));
    match shape {
        Shape::Month => {
            let first = capture_time::FloatingDate::new(day.year, day.month, 1).ok_or_else(bad)?;
            let last = capture_time::FloatingDate::new(
                day.year,
                day.month,
                month_length(day.year, day.month),
            )
            .ok_or_else(bad)?;
            Ok(Span {
                from: shift(first, -(weekday(first) as i64)).ok_or_else(bad)?,
                to: shift(last, 6 - weekday(last) as i64).ok_or_else(bad)?,
                first,
                last,
                title: format!("{} {}", MONTHS[day.month as usize - 1], day.year),
                previous: months_away(day, -1).ok_or_else(bad)?,
                next: months_away(day, 1).ok_or_else(bad)?,
            })
        }
        Shape::Week => {
            let monday = shift(day, -(weekday(day) as i64)).ok_or_else(bad)?;
            let sunday = shift(monday, 6).ok_or_else(bad)?;
            let title = if monday.month == sunday.month {
                format!(
                    "{} – {} {} {}",
                    monday.day,
                    sunday.day,
                    &MONTHS[sunday.month as usize - 1][..3],
                    sunday.year
                )
            } else {
                format!(
                    "{} {} – {} {} {}",
                    monday.day,
                    &MONTHS[monday.month as usize - 1][..3],
                    sunday.day,
                    &MONTHS[sunday.month as usize - 1][..3],
                    sunday.year
                )
            };
            Ok(Span {
                from: monday,
                to: sunday,
                first: monday,
                last: sunday,
                title,
                previous: shift(day, -7).ok_or_else(bad)?,
                next: shift(day, 7).ok_or_else(bad)?,
            })
        }
    }
}
