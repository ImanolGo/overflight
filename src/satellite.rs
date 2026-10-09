//! Satellites from Two-Line Elements, propagated with SGP4.
//!
//! TLEs come from Celestrak by default; a small recorded set is embedded so
//! `--demo` (and the tests) work offline. Positions are TEME from SGP4, rotated
//! into the observer's East-North-Up frame.

use anyhow::{Context, Result};
use sgp4::chrono::NaiveDateTime;
use sgp4::{Constants, Elements};

use crate::geo::{self, GeoPoint};

/// The default Celestrak group: crewed stations, including the ISS.
pub const DEFAULT_GROUP: &str = "stations";
/// Ignore anything past this many satellites, to bound the work.
const MAX_SATELLITES: usize = 250;

/// UTC datetime for a Unix timestamp, if it is representable.
#[must_use]
pub fn datetime(unix_seconds: i64) -> Option<NaiveDateTime> {
    sgp4::chrono::DateTime::from_timestamp(unix_seconds, 0).map(|datetime| datetime.naive_utc())
}

/// A satellite and its precomputed SGP4 constants.
#[derive(Debug)]
pub struct Satellite {
    pub name: String,
    elements: Elements,
    constants: Constants,
}

/// Where a satellite is in the observer's sky.
#[derive(Debug, Clone)]
pub struct SatellitePosition {
    pub name: String,
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
    pub altitude_km: f64,
}

impl Satellite {
    /// Build a satellite from a name and the two TLE lines.
    pub fn from_tle(name: String, line1: &[u8], line2: &[u8]) -> Result<Self> {
        let elements = Elements::from_tle(Some(name.clone()), line1, line2)
            .with_context(|| format!("parsing TLE for {name}"))?;
        let constants = Constants::from_elements(&elements)
            .with_context(|| format!("computing orbit for {name}"))?;
        Ok(Self {
            name,
            elements,
            constants,
        })
    }

    /// Propagate to `time` and locate the satellite from `observer`.
    #[must_use]
    pub fn position(&self, time: NaiveDateTime, observer: GeoPoint) -> Option<SatellitePosition> {
        let minutes = self.elements.datetime_to_minutes_since_epoch(&time).ok()?;
        let prediction = self.constants.propagate(minutes).ok()?;
        let [x, y, z] = prediction.position; // km, TEME

        // TEME -> ECEF, via Greenwich mean sidereal time.
        let julian_day = time.and_utc().timestamp() as f64 / 86_400.0 + 2_440_587.5;
        let gmst = (280.460_618_37 + 360.985_647_366_29 * (julian_day - 2_451_545.0)).to_radians();
        let (sin_g, cos_g) = gmst.sin_cos();
        let ecef = [
            (x * cos_g + y * sin_g) * 1000.0,
            (-x * sin_g + y * cos_g) * 1000.0,
            z * 1000.0,
        ];

        let [east, north, up] = geo::enu_from_ecef(observer, ecef);
        let ground = east.hypot(north);
        let radius = (ecef[0] * ecef[0] + ecef[1] * ecef[1] + ecef[2] * ecef[2]).sqrt();
        Some(SatellitePosition {
            name: self.name.clone(),
            azimuth_deg: east.atan2(north).to_degrees().rem_euclid(360.0),
            elevation_deg: up.atan2(ground).to_degrees(),
            altitude_km: radius / 1000.0 - 6_371.0,
        })
    }
}

/// Parse a Celestrak TLE file (three-line format).
pub fn parse(text: &str) -> Result<Vec<Satellite>> {
    let elements = sgp4::parse_3les(text).context("parsing TLEs")?;
    Ok(elements
        .into_iter()
        .take(MAX_SATELLITES)
        .filter_map(|elements| {
            let name = elements
                .object_name
                .clone()
                .unwrap_or_else(|| elements.norad_id.to_string());
            let constants = Constants::from_elements(&elements).ok()?;
            Some(Satellite {
                name,
                elements,
                constants,
            })
        })
        .collect())
}

/// The recorded TLEs bundled with the binary, for `--demo` and tests.
pub fn embedded() -> Result<Vec<Satellite>> {
    parse(include_str!("../fixtures/satellites.tle"))
}

/// Fetch the latest TLEs for a Celestrak group.
pub fn fetch(client: &reqwest::blocking::Client, group: &str) -> Result<Vec<Satellite>> {
    let url = format!("https://celestrak.org/NORAD/elements/gp.php?GROUP={group}&FORMAT=tle");
    let text = client
        .get(&url)
        .send()
        .context("Celestrak request")?
        .error_for_status()
        .context("Celestrak returned an error")?
        .text()
        .context("reading Celestrak response")?;
    parse(&text)
}

#[cfg(test)]
mod tests {
    use sgp4::chrono::Duration;

    use super::*;

    #[test]
    fn parses_the_embedded_tles() {
        let satellites = embedded().unwrap();
        assert!(!satellites.is_empty());
        assert!(
            satellites
                .iter()
                .any(|satellite| satellite.name.contains("ISS")),
            "expected the ISS in the embedded TLEs"
        );
    }

    #[test]
    fn the_iss_is_at_a_plausible_altitude_and_moves() {
        let satellites = embedded().unwrap();
        let iss = satellites
            .iter()
            .find(|satellite| satellite.name.contains("ISS"))
            .unwrap();
        let observer = GeoPoint::new(51.47, -0.4543, 0.0);
        let epoch = iss.elements.datetime;

        let first = iss.position(epoch, observer).unwrap();
        assert!(
            (300.0..500.0).contains(&first.altitude_km),
            "ISS altitude {} km",
            first.altitude_km
        );

        // Half an orbit later it should have moved substantially across the sky.
        let later = iss
            .position(epoch + Duration::minutes(46), observer)
            .unwrap();
        let moved = (later.azimuth_deg - first.azimuth_deg)
            .rem_euclid(360.0)
            .min((first.azimuth_deg - later.azimuth_deg).rem_euclid(360.0));
        assert!(moved > 20.0 || (later.elevation_deg - first.elevation_deg).abs() > 20.0);
    }

    #[test]
    fn a_distant_time_is_skipped_not_fatal() {
        let satellites = embedded().unwrap();
        let iss = &satellites[0];
        let observer = GeoPoint::new(0.0, 0.0, 0.0);
        // Ten years from the epoch: SGP4 may refuse, which is fine.
        let far = iss.elements.datetime + Duration::days(3650);
        let _ = iss.position(far, observer);
    }
}
