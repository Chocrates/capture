//! How long the overlay takes to appear.
//!
//! # Instrumented permanently, not tested once
//!
//! Capture latency is the one number this design cannot compromise on: the
//! gesture has to feel like it already happened, and a capture path that has
//! become slow is one that quietly stops being used. That degradation is
//! gradual — a font cache change, a slower disk, a compositor update — so a
//! measurement taken once during development says nothing about the machine six
//! months later.
//!
//! So it is measured on every capture, forever, and kept locally.
//!
//! # What this can and cannot see
//!
//! It measures from **the show request arriving** to **the first frame drawn**.
//! That is the part inside this process.
//!
//! The number that actually matters is physical keypress to visible cursor, and
//! it includes things no in-process timer can observe: the compositor noticing
//! the key, matching it against a binding, starting a process, that process
//! connecting to a socket, and — at the far end — the compositor compositing a
//! frame and a display showing it. Some of those happen before this code runs
//! and some after it stops.
//!
//! Reporting the in-process figure as though it were the whole gesture would be
//! the most flattering possible lie, so it is labelled for what it is. The
//! end-to-end number gets measured externally, once, and written down.
//!
//! # Buckets, not samples
//!
//! A histogram rather than a list of timings: it answers "is this usually fast,
//! and how bad is the tail" in a fixed amount of space, and the tail is the
//! interesting part. An average would hide exactly the occasional half-second
//! that makes a gesture feel unreliable.

/// Upper bounds, in milliseconds. The last bucket catches everything above.
///
/// Fine near the bottom because that is where the interesting distinctions are
/// — 8 ms and 60 ms feel completely different — and coarse at the top because
/// past a point the only fact that matters is "far too slow".
pub const BOUNDS: &[u64] = &[2, 4, 8, 16, 32, 64, 128, 256, 512, 1024];

/// Counts per bucket, plus one for everything slower than the last bound.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Histogram {
    counts: Vec<u64>,
}

impl Default for Histogram {
    fn default() -> Self {
        Histogram {
            counts: vec![0; BOUNDS.len() + 1],
        }
    }
}

impl Histogram {
    /// Add one measurement.
    pub fn record(&mut self, ms: u64) {
        let i = BOUNDS.iter().position(|b| ms <= *b).unwrap_or(BOUNDS.len());
        self.counts[i] += 1;
    }

    /// How many measurements have been taken.
    pub fn total(&self) -> u64 {
        self.counts.iter().sum()
    }

    /// The bucket bound at or below which the given share of samples fall.
    ///
    /// Reported as "at or under N ms" rather than as a precise figure, because
    /// a bucket is all the resolution there is and quoting a median of 7.3 ms
    /// from bucketed data would be inventing precision.
    pub fn quantile(&self, share: f64) -> Option<u64> {
        let total = self.total();
        if total == 0 {
            return None;
        }
        let target = (total as f64 * share).ceil() as u64;
        let mut seen = 0;
        for (i, c) in self.counts.iter().enumerate() {
            seen += c;
            if seen >= target {
                return Some(BOUNDS.get(i).copied().unwrap_or(u64::MAX));
            }
        }
        None
    }

    /// A line for a status page, or `None` if nothing has been measured.
    ///
    /// Names what was measured. This is the in-process portion, and calling it
    /// "capture latency" would claim the whole gesture.
    pub fn summary(&self) -> Option<String> {
        let total = self.total();
        if total == 0 {
            return None;
        }
        let p50 = self.quantile(0.50)?;
        let p95 = self.quantile(0.95)?;
        Some(format!(
            "{total} shown, half within {p50}ms, 95% within {}",
            if p95 == u64::MAX {
                format!("over {}ms", BOUNDS[BOUNDS.len() - 1])
            } else {
                format!("{p95}ms")
            }
        ))
    }

    /// The raw counts, for storage.
    pub fn counts(&self) -> &[u64] {
        &self.counts
    }

    /// Rebuild from stored counts, ignoring anything the wrong shape.
    ///
    /// A histogram whose buckets changed between versions is not convertible —
    /// the samples are gone and only their tallies remain — so it starts again
    /// rather than pretending the old counts mean the same thing.
    pub fn from_counts(counts: &[u64]) -> Histogram {
        if counts.len() != BOUNDS.len() + 1 {
            return Histogram::default();
        }
        Histogram {
            counts: counts.to_vec(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_histogram_reports_nothing() {
        // Not "0 ms", which would read as instant.
        let h = Histogram::default();
        assert_eq!(h.total(), 0);
        assert_eq!(h.quantile(0.5), None);
        assert_eq!(h.summary(), None);
    }

    #[test]
    fn a_measurement_lands_in_the_first_bucket_it_fits() {
        let mut h = Histogram::default();
        h.record(1);
        assert_eq!(h.quantile(0.5), Some(2));

        let mut h = Histogram::default();
        h.record(3);
        assert_eq!(h.quantile(0.5), Some(4));
    }

    #[test]
    fn a_boundary_belongs_to_its_own_bucket() {
        // `<=`, so a measurement exactly on a bound is not pushed up a bucket
        // and made to look worse than it was.
        let mut h = Histogram::default();
        h.record(4);
        assert_eq!(h.quantile(0.5), Some(4));
    }

    #[test]
    fn anything_slower_than_the_last_bound_is_still_counted() {
        // Dropping it would make the histogram look better the worse things
        // got, which is the opposite of what a tail measurement is for.
        let mut h = Histogram::default();
        h.record(60_000);
        assert_eq!(h.total(), 1);
        assert!(h.summary().unwrap().contains("over 1024ms"));
    }

    #[test]
    fn the_tail_is_visible_rather_than_averaged_away() {
        // The reason this is a histogram. Ninety-nine fast captures and one
        // that took a second average out to something reassuring, and the
        // gesture still feels unreliable because of the one.
        let mut h = Histogram::default();
        for _ in 0..99 {
            h.record(2);
        }
        h.record(900);

        assert_eq!(h.quantile(0.50), Some(2));
        assert_eq!(h.quantile(0.999), Some(1024));
    }

    #[test]
    fn a_summary_says_what_it_measured() {
        // "Capture latency" would claim the whole gesture, most of which
        // happens outside this process.
        let mut h = Histogram::default();
        h.record(5);
        let s = h.summary().unwrap();
        assert!(s.contains("shown"), "{s}");
        assert!(s.contains("1 shown"), "{s}");
    }

    #[test]
    fn counts_survive_a_round_trip() {
        let mut h = Histogram::default();
        h.record(1);
        h.record(50);
        h.record(5_000);
        assert_eq!(Histogram::from_counts(h.counts()), h);
    }

    #[test]
    fn counts_from_a_different_shape_are_discarded() {
        // The buckets changed between versions. The samples are gone and only
        // tallies remain, so there is nothing to convert — starting again is
        // the only honest option.
        let odd = vec![1, 2, 3];
        assert_eq!(Histogram::from_counts(&odd).total(), 0);
    }

    #[test]
    fn every_bucket_can_be_reached() {
        // A bound nothing can land in would silently never be reported.
        for (i, b) in BOUNDS.iter().enumerate() {
            let mut h = Histogram::default();
            h.record(*b);
            assert_eq!(h.quantile(1.0), Some(*b), "bucket {i} ({b}ms) is reachable");
        }
    }
}
