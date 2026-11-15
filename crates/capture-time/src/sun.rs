//! Sunrise and sunset (alarms-at-sunrise-and-sunset D5).
//!
//! # The method
//!
//! NOAA's solar calculator method — the one in its published spreadsheet,
//! itself from Meeus, *Astronomical Algorithms* — with the standard refraction
//! and solar-disc allowance: the sun "rises" when its centre is 0.833° below
//! the horizon. Accurate to about a minute below the polar circles, which is
//! finer than anything else in a wake-up alarm.
//!
//! # Floats, and why that is allowed here
//!
//! Nothing computed here is stored, hashed or used to name anything. An
//! occurrence of a sun series is named by its date (D3), so a build whose
//! trigonometry differs in the last bit — or a better algorithm next year —
//! moves a ring by at most a minute and renames nothing.
//!
//! # Pure of its inputs
//!
//! A date, a zone, a place and which event. No clock, no location lookup:
//! where the person is comes in as a value, and only the phone ever has one.

use crate::{FloatingDate, Zone, at_local};

/// Which crossing of the horizon.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SunEvent {
    /// The sun's upper edge appears.
    Rise,
    /// The sun's upper edge goes.
    Set,
}

/// A day on which the sun does not cross the horizon at that place.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Polar {
    /// The sun stays below all day: no sunrise and no sunset.
    Night,
    /// The sun stays above all day: no sunset and no sunrise.
    Day,
}

const MS_PER_DAY: f64 = 86_400_000.0;
const MS_PER_MIN: f64 = 60_000.0;
/// Julian day of the Unix epoch.
const JD_UNIX: f64 = 2_440_587.5;
/// The zenith of the sun's centre at rise and set: 90° plus refraction and
/// half the disc.
const ZENITH: f64 = 90.833;

fn rad(d: f64) -> f64 {
    d * core::f64::consts::PI / 180.0
}

fn deg(r: f64) -> f64 {
    r * 180.0 / core::f64::consts::PI
}

/// Where the sun is at one instant: its declination (degrees) and the
/// equation of time (minutes).
fn position(ms: f64) -> (f64, f64) {
    let jd = ms / MS_PER_DAY + JD_UNIX;
    let t = (jd - 2_451_545.0) / 36_525.0;

    let l0 = (280.466_46 + t * (36_000.769_83 + t * 0.000_303_2)) % 360.0;
    let m = 357.529_11 + t * (35_999.050_29 - 0.000_153_7 * t);
    let e = 0.016_708_634 - t * (0.000_042_037 + 0.000_000_126_7 * t);
    let c = libm::sin(rad(m)) * (1.914_602 - t * (0.004_817 + 0.000_014 * t))
        + libm::sin(rad(2.0 * m)) * (0.019_993 - 0.000_101 * t)
        + libm::sin(rad(3.0 * m)) * 0.000_289;
    let true_long = l0 + c;
    let omega = 125.04 - 1_934.136 * t;
    let app_long = true_long - 0.005_69 - 0.004_78 * libm::sin(rad(omega));
    let mean_obliq =
        23.0 + (26.0 + (21.448 - t * (46.815 + t * (0.000_59 - t * 0.001_813))) / 60.0) / 60.0;
    let obliq = mean_obliq + 0.002_56 * libm::cos(rad(omega));
    let decl = deg(libm::asin(libm::sin(rad(obliq)) * libm::sin(rad(app_long))));

    let y = libm::tan(rad(obliq / 2.0)) * libm::tan(rad(obliq / 2.0));
    let eq = 4.0
        * deg(y * libm::sin(2.0 * rad(l0)) - 2.0 * e * libm::sin(rad(m))
            + 4.0 * e * y * libm::sin(rad(m)) * libm::cos(2.0 * rad(l0))
            - 0.5 * y * y * libm::sin(4.0 * rad(l0))
            - 1.25 * e * e * libm::sin(2.0 * rad(m)));
    (decl, eq)
}

/// The hour angle of rise/set in degrees, or which polar case it is.
fn hour_angle(lat: f64, decl: f64) -> Result<f64, Polar> {
    let x = libm::cos(rad(ZENITH)) / (libm::cos(rad(lat)) * libm::cos(rad(decl)))
        - libm::tan(rad(lat)) * libm::tan(rad(decl));
    if x.is_nan() {
        // Only at a pole exactly, where cos(lat) is zero and the sun circles
        // at one height all day. Which side of the horizon it circles on is
        // the declination's sign against the pole's.
        return Err(if (lat > 0.0) == (decl > 0.0) {
            Polar::Day
        } else {
            Polar::Night
        });
    }
    if x > 1.0 {
        Err(Polar::Night)
    } else if x < -1.0 {
        Err(Polar::Day)
    } else {
        Ok(deg(libm::acos(x)))
    }
}

/// When the sun rises or sets on a local date, at a place.
///
/// `lat_e6` and `lon_e6` are millionths of a degree, north and east positive.
/// Returns UTC milliseconds rounded to the minute, or [`Polar`] when the sun
/// does not cross the horizon that day.
///
/// "That day" is the solar day whose noon falls nearest the date's local noon.
/// Near the polar circles a summer sunset can therefore land a little after
/// local midnight, on the next calendar day — which is when it happens.
pub fn sun(
    date: FloatingDate,
    zone: Zone,
    lat_e6: i32,
    lon_e6: i32,
    event: SunEvent,
) -> Result<i64, Polar> {
    let lat = (f64::from(lat_e6) / 1e6).clamp(-90.0, 90.0);
    let lon = f64::from(lon_e6) / 1e6;
    // A date the zone cannot place has no noon; treat it as UTC noon, which
    // is never more than fourteen hours out and only affects which solar day.
    let local_noon = at_local(date, 12, 0, zone)
        .or_else(|| Zone::new("UTC").and_then(|utc| at_local(date, 12, 0, utc)))
        .unwrap_or(0) as f64;

    // Solar noon nearest local noon: the UTC day it falls in is the one whose
    // midnight plus (720 - 4·lon - eq) minutes is closest.
    let (_, eq0) = position(local_noon);
    let noon_offset = (720.0 - 4.0 * lon - eq0) * MS_PER_MIN;
    let mut midnight = libm::floor(local_noon / MS_PER_DAY) * MS_PER_DAY;
    while midnight + noon_offset - local_noon > MS_PER_DAY / 2.0 {
        midnight -= MS_PER_DAY;
    }
    while local_noon - (midnight + noon_offset) > MS_PER_DAY / 2.0 {
        midnight += MS_PER_DAY;
    }

    // Two refinements: the sun's position at the event, not at noon, moves
    // the answer by up to a minute in spring and autumn at high latitude.
    let mut at = midnight + noon_offset;
    for _ in 0..3 {
        let (decl, eq) = position(at);
        let ha = hour_angle(lat, decl)?;
        let sign = match event {
            SunEvent::Rise => 1.0,
            SunEvent::Set => -1.0,
        };
        at = midnight + (720.0 - 4.0 * (lon + sign * ha) - eq) * MS_PER_MIN;
    }
    let minute = libm::round(at / MS_PER_MIN);
    Ok((minute * MS_PER_MIN) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u8, day: u8) -> FloatingDate {
        FloatingDate::new(y, m, day).unwrap()
    }

    fn z(name: &str) -> Zone {
        Zone::new(name).unwrap()
    }

    fn day_length_min(date: FloatingDate, zone: Zone, lat: i32, lon: i32) -> i64 {
        let rise = sun(date, zone, lat, lon, SunEvent::Rise).unwrap();
        let set = sun(date, zone, lat, lon, SunEvent::Set).unwrap();
        (set - rise) / 60_000
    }

    #[test]
    fn the_equator_has_a_twelve_hour_day_plus_refraction_all_year() {
        // At the equator the day is 12 h plus about 7 minutes of refraction
        // and disc, whatever the season, to within a couple of minutes.
        for (m, day) in [(1, 15), (3, 20), (6, 21), (9, 22), (12, 21)] {
            let len = day_length_min(d(2026, m, day), z("UTC"), 0, 0);
            assert!((725..=730).contains(&len), "{m}-{day}: {len} min");
        }
    }

    #[test]
    fn solar_noon_falls_between_rise_and_set() {
        // At Greenwich, solar noon is 12:00 UTC minus the equation of time,
        // which ranges over about ±16 minutes in a year.
        for (m, day) in [(2, 11), (5, 14), (7, 26), (11, 3)] {
            let date = d(2026, m, day);
            let rise = sun(date, z("Europe/London"), 51_477_000, 0, SunEvent::Rise).unwrap();
            let set = sun(date, z("Europe/London"), 51_477_000, 0, SunEvent::Set).unwrap();
            let noon = (rise + set) / 2;
            let utc_noon = at_local(date, 12, 0, z("UTC")).unwrap();
            assert!((noon - utc_noon).abs() <= 17 * 60_000, "{m}-{day}");
        }
    }

    #[test]
    fn the_hemispheres_mirror_at_the_solstices() {
        // 52°N in June and 52°S in December have days the same length, give or
        // take the eccentricity of the orbit (a few minutes).
        let north = day_length_min(d(2026, 6, 21), z("UTC"), 52_000_000, 0);
        let south = day_length_min(d(2026, 12, 21), z("UTC"), -52_000_000, 0);
        assert!((north - south).abs() <= 6, "{north} vs {south}");
        assert!(
            (990..=1010).contains(&north),
            "52°N midsummer is about 16h40m: {north}"
        );
    }

    #[test]
    fn past_the_polar_circle_the_sun_does_not_cross() {
        // 78°N (Svalbard): midnight sun in June, polar night in December.
        let svalbard = z("Arctic/Longyearbyen");
        for e in [SunEvent::Rise, SunEvent::Set] {
            assert_eq!(
                sun(d(2026, 6, 21), svalbard, 78_220_000, 15_650_000, e),
                Err(Polar::Day)
            );
            assert_eq!(
                sun(d(2026, 12, 21), svalbard, 78_220_000, 15_650_000, e),
                Err(Polar::Night)
            );
        }
        // Just inside the circle at midsummer the sun still sets.
        assert!(sun(d(2026, 6, 21), z("UTC"), 65_000_000, 0, SunEvent::Set).is_ok());
        // And the poles themselves.
        assert_eq!(
            sun(d(2026, 6, 21), z("UTC"), 90_000_000, 0, SunEvent::Rise),
            Err(Polar::Day)
        );
        assert_eq!(
            sun(d(2026, 6, 21), z("UTC"), -90_000_000, 0, SunEvent::Rise),
            Err(Polar::Night)
        );
    }

    #[test]
    fn rise_comes_before_set_and_both_are_whole_minutes() {
        let r = sun(
            d(2026, 11, 14),
            z("America/Chicago"),
            41_880_000,
            -87_630_000,
            SunEvent::Rise,
        )
        .unwrap();
        let s = sun(
            d(2026, 11, 14),
            z("America/Chicago"),
            41_880_000,
            -87_630_000,
            SunEvent::Set,
        )
        .unwrap();
        assert!(r < s);
        assert_eq!(r % 60_000, 0);
        assert_eq!(s % 60_000, 0);
    }
}
