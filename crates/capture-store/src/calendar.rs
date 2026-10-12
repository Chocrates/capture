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
