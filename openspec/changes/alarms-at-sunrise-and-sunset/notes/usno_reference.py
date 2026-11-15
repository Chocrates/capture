# USNO "Almanac for Computers" (1990) sunrise/sunset, as in Ed Williams'
# aviation formulary: an independent formulation of the same quantity, used
# to cross-check capture-time's NOAA/Meeus implementation.
import math, datetime
def usno(y, m, d, lat, lon, rise):
    N = datetime.date(y, m, d).timetuple().tm_yday
    lh = lon / 15
    t = N + ((6 - lh) / 24 if rise else (18 - lh) / 24)
    M = 0.9856 * t - 3.289
    L = (M + 1.916 * math.sin(math.radians(M)) + 0.020 * math.sin(math.radians(2 * M)) + 282.634) % 360
    RA = math.degrees(math.atan(0.91764 * math.tan(math.radians(L)))) % 360
    RA = (RA + (math.floor(L / 90) * 90 - math.floor(RA / 90) * 90)) / 15
    sd = 0.39782 * math.sin(math.radians(L)); cd = math.cos(math.asin(sd))
    ch = (math.cos(math.radians(90.833)) - sd * math.sin(math.radians(lat))) / (cd * math.cos(math.radians(lat)))
    if ch > 1 or ch < -1: return None
    H = (360 - math.degrees(math.acos(ch))) if rise else math.degrees(math.acos(ch))
    T = H / 15 + RA - 0.06571 * t - 6.622
    UT = (T - lh) % 24
    # Minutes after UTC midnight of the date; the caller places the day.
    return UT * 60
cities = [("quito", "America/Guayaquil", -0.1807, -78.4678),
          ("singapore", "Asia/Singapore", 1.3521, 103.8198),
          ("tokyo", "Asia/Tokyo", 35.6762, 139.6503),
          ("sydney", "Australia/Sydney", -33.8688, 151.2093),
          ("london", "Europe/London", 51.5074, -0.1278),
          ("chicago", "America/Chicago", 41.8781, -87.6298),
          ("punta-arenas", "America/Punta_Arenas", -53.1638, -70.9171),
          ("reykjavik", "Atlantic/Reykjavik", 64.1466, -21.9426)]
dates = [(2026, 3, 20), (2026, 6, 21), (2026, 9, 22), (2026, 12, 21)]
for name, tz, lat, lon in cities:
    for (y, m, d) in dates:
        r = usno(y, m, d, lat, lon, True); s = usno(y, m, d, lat, lon, False)
        f = lambda v: "None" if v is None else f"Some({v:.1f})"
        print(f'    ("{name}", "{tz}", {round(lat*1e6)}, {round(lon*1e6)}, ({y}, {m}, {d}), {f(r)}, {f(s)}),')
