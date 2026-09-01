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
