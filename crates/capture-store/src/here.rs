//! Where this device last was, for working out sunrise and sunset
//! (alarms-at-sunrise-and-sunset D1).
//!
//! # Local, and never an event
//!
//! Kept in `ops.db`, which is this device's own and is never synced. An
//! append-only log that never forgets is the one place a movement history must
//! not be written: nothing short of redaction takes it back. So a sun alarm
//! stores which sun and an offset, and the phone supplies where, at due time.
//!
//! # Rounded here, not on the phone
//!
//! To 0.01° — about a kilometre, over which sunrise moves well under a minute.
//! Rounded in Rust so the phone hands over whatever its platform gave and
//! decides nothing, and so no finer position is ever at rest on disk.

use crate::{Error, Result, ops::Ops};

const KEY: &str = "here";

/// The last place this device reported, and when.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Here {
    /// Millionths of a degree north, a multiple of 10 000.
    pub lat_e6: i32,
    /// Millionths of a degree east, a multiple of 10 000.
    pub lon_e6: i32,
    /// When the fix was taken, UTC milliseconds.
    pub fix_ms: i64,
}

/// Older than this, a computed time says where it was computed for.
pub const STALE_MS: i64 = 24 * 3_600_000;

fn round(deg: f64) -> i32 {
    (deg * 100.0).round() as i32 * 10_000
}

impl Here {
    /// From what a platform reports, rounded to 0.01°.
    pub fn from_degrees(lat: f64, lon: f64, fix_ms: i64) -> Result<Here> {
        if !(lat.is_finite() && lon.is_finite()) || lat.abs() > 90.0 || lon.abs() > 180.0 {
            return Err(Error::Usage(format!("not a place on Earth: {lat}, {lon}")));
        }
        Ok(Here {
            lat_e6: round(lat),
            lon_e6: round(lon),
            fix_ms,
        })
    }

    /// How old the fix is at `now`.
    pub fn age_ms(&self, now: i64) -> i64 {
        (now - self.fix_ms).max(0)
    }

    /// Whether a time computed from it should say where it was computed for.
    pub fn is_stale(&self, now: i64) -> bool {
        self.age_ms(now) > STALE_MS
    }
}

/// Record where this device is. A fix older than the one held is ignored.
pub fn set(ops: &Ops, here: Here) -> Result<()> {
    if get(ops)?.is_some_and(|held| held.fix_ms > here.fix_ms) {
        return Ok(());
    }
    ops.set(
        KEY,
        &format!("{},{},{}", here.lat_e6, here.lon_e6, here.fix_ms),
    )
}

/// Where this device last was, if it has ever said.
pub fn get(ops: &Ops) -> Result<Option<Here>> {
    let Some(v) = ops.get(KEY)? else {
        return Ok(None);
    };
    let mut parts = v.split(',').map(str::parse::<i64>);
    match (parts.next(), parts.next(), parts.next()) {
        (Some(Ok(lat)), Some(Ok(lon)), Some(Ok(fix))) => Ok(Some(Here {
            lat_e6: lat as i32,
            lon_e6: lon as i32,
            fix_ms: fix,
        })),
        // Unreadable is the same as never told: nothing is guessed.
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fix_is_rounded_to_a_hundredth_of_a_degree() {
        let h = Here::from_degrees(41.878_113, -87.629_799, 5).unwrap();
        assert_eq!((h.lat_e6, h.lon_e6), (41_880_000, -87_630_000));
        assert!(Here::from_degrees(f64::NAN, 0.0, 0).is_err());
        assert!(Here::from_degrees(91.0, 0.0, 0).is_err());
    }
}
