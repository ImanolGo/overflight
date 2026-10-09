//! Solar position, using the NOAA Global Monitoring Laboratory formulas.
//!
//! Reference: NOAA Solar Calculator and its spreadsheet,
//! <https://gml.noaa.gov/grad/solcalc/>. Values are cross-checked against the
//! MET Norway sunrise API in the tests.

/// Seconds in a day.
const SECONDS_PER_DAY: i64 = 86_400;
/// Julian day of the J2000.0 epoch (2000-01-01 12:00 UTC).
const J2000: f64 = 2_451_545.0;

/// A civil UTC date and time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utc {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Utc {
    /// Build a UTC timestamp.
    #[must_use]
    pub const fn new(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> Self {
        Self {
            year,
            month,
            day,
            hour,
            minute,
            second,
        }
    }

    /// Convert a Unix timestamp (seconds since 1970-01-01 00:00 UTC).
    #[must_use]
    pub const fn from_unix_seconds(seconds: i64) -> Self {
        let days = seconds.div_euclid(SECONDS_PER_DAY);
        let secs = seconds.rem_euclid(SECONDS_PER_DAY);
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: (secs / 3600) as u32,
            minute: ((secs % 3600) / 60) as u32,
            second: (secs % 60) as u32,
        }
    }

    /// Convert to a Unix timestamp.
    #[must_use]
    pub const fn to_unix_seconds(self) -> i64 {
        days_from_civil(self.year, self.month, self.day) * SECONDS_PER_DAY
            + self.hour as i64 * 3600
            + self.minute as i64 * 60
            + self.second as i64
    }

    /// Julian day number for this instant.
    #[must_use]
    pub fn julian_day(self) -> f64 {
        let (mut year, mut month) = (self.year, self.month);
        if month <= 2 {
            year -= 1;
            month += 12;
        }
        let a = f64::from(year) / 100.0;
        let a = a.floor();
        let b = 2.0 - a + (a / 4.0).floor();
        let day = f64::from(self.day)
            + (f64::from(self.hour)
                + f64::from(self.minute) / 60.0
                + f64::from(self.second) / 3600.0)
                / 24.0;
        (365.25 * (f64::from(year) + 4716.0)).floor()
            + (30.6001 * f64::from(month + 1)).floor()
            + day
            + b
            - 1524.5
    }
}

/// Civil date from a count of days since the Unix epoch (Howard Hinnant's
/// `civil_from_days`).
const fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let year = if month <= 2 { year + 1 } else { year };
    (year as i32, month as u32, day as u32)
}

/// Days since the Unix epoch from a civil date (Howard Hinnant's
/// `days_from_civil`).
const fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year } as i64;
    let month = month as i64;
    let day = day as i64;
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400; // [0, 399]
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Solar elevation above the horizon, in degrees, including atmospheric
/// refraction (as the NOAA calculator reports it).
#[must_use]
pub fn solar_elevation_deg(lat_deg: f64, lon_deg: f64, time: Utc) -> f64 {
    let t = (time.julian_day() - J2000) / 36_525.0;

    let l0 = (280.46646 + t * (36_000.769_83 + t * 0.000_303_2)).rem_euclid(360.0);
    let m = 357.52911 + t * (35_999.050_29 - 0.000_153_7 * t);
    let m_rad = m.to_radians();
    let eccentricity = 0.016_708_634 - t * (0.000_042_037 + 0.000_000_126_7 * t);

    let center = m_rad.sin() * (1.914_602 - t * (0.004_817 + 0.000_014 * t))
        + (2.0 * m_rad).sin() * (0.019_993 - 0.000_101 * t)
        + (3.0 * m_rad).sin() * 0.000_289;
    let true_longitude = l0 + center;

    let omega = 125.04 - 1934.136 * t;
    let omega_rad = omega.to_radians();
    let apparent_longitude = true_longitude - 0.005_69 - 0.004_78 * omega_rad.sin();

    let mean_obliquity =
        23.0 + (26.0 + (21.448 - t * (46.815 + t * (0.000_59 - t * 0.001_813))) / 60.0) / 60.0;
    let obliquity = mean_obliquity + 0.002_56 * omega_rad.cos();
    let declination = (obliquity.to_radians().sin() * apparent_longitude.to_radians().sin()).asin();

    // Equation of time, in minutes.
    let y = (obliquity / 2.0).to_radians().tan().powi(2);
    let l0_rad = l0.to_radians();
    let equation_of_time = 4.0
        * (y * (2.0 * l0_rad).sin() - 2.0 * eccentricity * m_rad.sin()
            + 4.0 * eccentricity * y * m_rad.sin() * (2.0 * l0_rad).cos()
            - 0.5 * y * y * (4.0 * l0_rad).sin()
            - 1.25 * eccentricity * eccentricity * (2.0 * m_rad).sin())
        .to_degrees();

    let minutes =
        f64::from(time.hour) * 60.0 + f64::from(time.minute) + f64::from(time.second) / 60.0;
    let true_solar_time = (minutes + equation_of_time + 4.0 * lon_deg).rem_euclid(1440.0);
    let hour_angle = true_solar_time / 4.0 - 180.0;

    let lat_rad = lat_deg.to_radians();
    let cosine_zenith = lat_rad.sin() * declination.sin()
        + lat_rad.cos() * declination.cos() * hour_angle.to_radians().cos();
    let elevation = 90.0 - cosine_zenith.clamp(-1.0, 1.0).acos().to_degrees();
    elevation + refraction_deg(elevation)
}

/// Atmospheric refraction correction in degrees, from the NOAA spreadsheet.
fn refraction_deg(elevation_deg: f64) -> f64 {
    let arcseconds = if elevation_deg > 85.0 {
        0.0
    } else if elevation_deg > 5.0 {
        let t = elevation_deg.to_radians().tan();
        58.1 / t - 0.07 / t.powi(3) + 0.000_086 / t.powi(5)
    } else if elevation_deg > -0.575 {
        1735.0
            + elevation_deg
                * (-518.2
                    + elevation_deg * (103.4 + elevation_deg * (-12.79 + elevation_deg * 0.711)))
    } else {
        -20.772 / elevation_deg.to_radians().tan()
    };
    arcseconds / 3600.0
}

/// Parse an RFC 3339 timestamp (e.g. `2024-06-20T11:08:00+02:00`) into Unix
/// seconds UTC. Fractional seconds are ignored.
pub fn parse_rfc3339_seconds(input: &str) -> anyhow::Result<i64> {
    let input = input.trim();
    let (date, rest) = input
        .split_once('T')
        .or_else(|| input.split_once('t'))
        .ok_or_else(|| {
            anyhow::anyhow!("expected an RFC 3339 timestamp like 2024-06-20T11:08:00Z")
        })?;

    let mut date_parts = date.split('-');
    let year: i32 = parse_field(date_parts.next(), "year")?;
    let month: u32 = parse_field(date_parts.next(), "month")?;
    let day: u32 = parse_field(date_parts.next(), "day")?;
    if date_parts.next().is_some() {
        anyhow::bail!("invalid date in {input:?}");
    }

    let (time, offset_seconds) = if rest.ends_with('Z') || rest.ends_with('z') {
        (&rest[..rest.len() - 1], 0)
    } else {
        let sign_at = rest
            .rfind(['+', '-'])
            .ok_or_else(|| anyhow::anyhow!("timestamp needs a 'Z' or numeric offset"))?;
        let (time, offset) = rest.split_at(sign_at);
        let sign = if offset.starts_with('-') { -1 } else { 1 };
        let mut offset_parts = offset[1..].split(':');
        let hours: i64 = parse_field(offset_parts.next(), "offset hours")?;
        let minutes: i64 = parse_field(offset_parts.next(), "offset minutes")?;
        (time, sign * (hours * 3600 + minutes * 60))
    };

    // Drop any fractional seconds.
    let time = time.split_once('.').map_or(time, |(whole, _)| whole);
    let mut time_parts = time.split(':');
    let hour: u32 = parse_field(time_parts.next(), "hour")?;
    let minute: u32 = parse_field(time_parts.next(), "minute")?;
    let second: u32 = parse_field(time_parts.next(), "second")?;

    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        anyhow::bail!("invalid date or time in {input:?}");
    }

    Ok(Utc::new(year, month, day, hour, minute, second).to_unix_seconds() - offset_seconds)
}

fn parse_field<T: std::str::FromStr>(field: Option<&str>, name: &str) -> anyhow::Result<T> {
    field
        .ok_or_else(|| anyhow::anyhow!("missing {name}"))?
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid {name}"))
}

#[cfg(test)]
mod tests {
    use approx::assert_abs_diff_eq;

    use super::*;

    #[test]
    fn unix_epoch_round_trips() {
        let t = Utc::from_unix_seconds(0);
        assert_eq!(t, Utc::new(1970, 1, 1, 0, 0, 0));
        assert_eq!(t.to_unix_seconds(), 0);
    }

    #[test]
    fn unix_timestamps_round_trip() {
        let t = Utc::from_unix_seconds(1_000_000_000);
        assert_eq!(t, Utc::new(2001, 9, 9, 1, 46, 40));
        assert_eq!(t.to_unix_seconds(), 1_000_000_000);
    }

    #[test]
    fn parses_rfc3339_timestamps() {
        assert_eq!(parse_rfc3339_seconds("1970-01-01T00:00:00Z").unwrap(), 0);
        assert_eq!(
            parse_rfc3339_seconds("2001-09-09T01:46:40Z").unwrap(),
            1_000_000_000
        );
        // Fractional seconds are ignored.
        assert_eq!(
            parse_rfc3339_seconds("2001-09-09T01:46:40.500Z").unwrap(),
            1_000_000_000
        );
        // A +02:00 offset is the same instant as 01:46:40Z.
        assert_eq!(
            parse_rfc3339_seconds("2001-09-09T03:46:40+02:00").unwrap(),
            1_000_000_000
        );
        assert_eq!(
            parse_rfc3339_seconds("2001-09-08T20:46:40-05:00").unwrap(),
            1_000_000_000
        );
    }

    #[test]
    fn rejects_malformed_timestamps() {
        assert!(parse_rfc3339_seconds("2024-06-20").is_err());
        assert!(parse_rfc3339_seconds("2024-13-40T00:00:00Z").is_err());
        assert!(parse_rfc3339_seconds("not a time").is_err());
    }

    #[test]
    fn julian_day_of_j2000() {
        let t = Utc::new(2000, 1, 1, 12, 0, 0);
        assert_abs_diff_eq!(t.julian_day(), 2_451_545.0, epsilon = 1e-9);
    }

    #[test]
    fn berlin_solar_noon_on_the_june_solstice() {
        // MET Norway sunrise API, 52.52N 13.40E, 2024-06-20: solar noon at
        // 11:08 UTC with a disc-centre elevation of 60.92 deg.
        let elevation = solar_elevation_deg(52.52, 13.40, Utc::new(2024, 6, 20, 11, 8, 0));
        assert_abs_diff_eq!(elevation, 60.92, epsilon = 0.2);
    }

    #[test]
    fn berlin_solar_midnight_is_below_the_horizon() {
        // Same source: solar midnight at 23:08 UTC, elevation -14.05 deg.
        let elevation = solar_elevation_deg(52.52, 13.40, Utc::new(2024, 6, 19, 23, 8, 0));
        assert_abs_diff_eq!(elevation, -14.05, epsilon = 0.2);
        assert!(elevation < 0.0);
    }

    #[test]
    fn north_pole_midnight_sun_sits_at_the_obliquity() {
        // At the pole on the June solstice the sun circles the horizon at
        // roughly the axial tilt, 23.44 deg.
        let elevation = solar_elevation_deg(90.0, 0.0, Utc::new(2024, 6, 20, 12, 0, 0));
        assert_abs_diff_eq!(elevation, 23.44, epsilon = 0.1);
    }
}
