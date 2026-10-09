//! Flight-data providers.
//!
//! Every source implements [`Provider`] and returns the same normalized
//! [`Aircraft`] type. All network access lives behind that trait; tests use the
//! fixture provider and never touch the network.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;

use crate::geo::{self, GeoPoint};

pub mod airplanes_live;
pub mod fixture;
pub mod local;
pub mod opensky;
pub mod readsb;

/// The kind of aircraft, from its ADS-B emitter category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AircraftKind {
    #[default]
    Plane,
    Helicopter,
    Glider,
    Balloon,
}

/// A single aircraft state, normalized across providers.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Aircraft {
    /// ICAO 24-bit hex address, lowercase.
    pub id: String,
    /// Callsign, trimmed; `None` if empty.
    pub callsign: Option<String>,
    pub registration: Option<String>,
    /// ICAO type code, e.g. `A20N`.
    pub type_code: Option<String>,
    /// Aircraft kind from the ADS-B emitter category.
    pub kind: AircraftKind,
    pub lat: f64,
    pub lon: f64,
    /// Geometric altitude if available, else barometric, in metres.
    pub alt_m: Option<f64>,
    pub on_ground: bool,
    pub ground_speed_ms: Option<f64>,
    /// True track, degrees clockwise from north.
    pub track_deg: Option<f64>,
    pub vertical_rate_ms: Option<f64>,
    /// Age of the position when the response was produced, seconds.
    pub position_age_s: f64,
    /// Transponder squawk code, if any.
    pub squawk: Option<String>,
    /// ADS-B emergency/priority status, if not `none`.
    pub emergency: Option<String>,
    /// Whether the database flags this as a military aircraft.
    pub military: bool,
    /// Whether the database flags this as interesting.
    pub interesting: bool,
}

impl Aircraft {
    /// The label to show for this aircraft: callsign, then registration, then id.
    #[must_use]
    pub fn label(&self) -> &str {
        self.callsign
            .as_deref()
            .or(self.registration.as_deref())
            .unwrap_or(&self.id)
    }

    /// Ground distance from an observer, in metres.
    #[must_use]
    pub fn ground_distance_m(&self, observer: GeoPoint) -> f64 {
        let target = GeoPoint::new(self.lat, self.lon, self.alt_m.unwrap_or(0.0));
        let [east, north, _] = geo::enu(observer, target);
        east.hypot(north)
    }

    /// A short reason this aircraft is unusual, if it is.
    #[must_use]
    pub fn unusual_reason(&self) -> Option<&'static str> {
        if self.emergency.is_some() {
            return Some("emergency");
        }
        match self.squawk.as_deref() {
            Some("7700") => return Some("emergency squawk 7700"),
            Some("7600") => return Some("radio failure squawk 7600"),
            Some("7500") => return Some("unlawful interference 7500"),
            _ => {}
        }
        if self.military {
            return Some("military");
        }
        if is_rare_type(self.type_code.as_deref()) {
            return Some("rare type");
        }
        if self.interesting {
            return Some("of interest");
        }
        None
    }
}

/// Type codes rare enough to be worth a nod.
fn is_rare_type(type_code: Option<&str>) -> bool {
    const RARE: [&str; 8] = [
        "A388", "B748", "A225", "AN124", "C5M", "B52H", "CONC", "A124",
    ];
    type_code.is_some_and(|code| RARE.contains(&code))
}

/// Where to look for aircraft.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Query {
    pub lat: f64,
    pub lon: f64,
    pub radius_km: f64,
}

impl Query {
    /// The observer's position.
    #[must_use]
    pub const fn observer(&self) -> GeoPoint {
        GeoPoint::new(self.lat, self.lon, 0.0)
    }

    /// Whether an aircraft lies within the query radius, by ground distance.
    #[must_use]
    pub fn contains(&self, aircraft: &Aircraft) -> bool {
        aircraft.ground_distance_m(self.observer()) <= self.radius_km * 1000.0
    }
}

/// A flight-data source.
pub trait Provider: Send {
    /// Human-readable source name, for the status line.
    fn name(&self) -> &'static str;
    /// The shortest interval the provider's terms allow between requests.
    fn min_interval(&self) -> Duration;
    /// Fetch the current aircraft near `query`.
    fn fetch(&mut self, query: &Query) -> Result<Vec<Aircraft>>;
}

/// `User-Agent` sent with every request, naming overflight and linking the repo.
pub const USER_AGENT: &str = concat!(
    "overflight/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/ImanolGo/overflight)"
);

/// Build the shared blocking HTTP client.
pub fn http_client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()?)
}

/// Saves each raw provider response to a directory, for building fixtures.
#[derive(Debug)]
pub struct Recorder {
    dir: PathBuf,
    count: usize,
}

impl Recorder {
    /// Record into `dir`, creating it if needed.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            count: 0,
        }
    }

    /// Save one raw response as `frame_0000.json`, `frame_0001.json`, ...
    pub fn record(&mut self, raw: &str) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(format!("frame_{:04}.json", self.count));
        std::fs::write(path, raw)?;
        self.count += 1;
        Ok(())
    }
}

/// Convert knots to metres per second.
pub(crate) fn knots_to_ms(knots: f64) -> f64 {
    knots * 0.514_444
}

/// Convert feet to metres.
pub(crate) fn feet_to_m(feet: f64) -> f64 {
    feet * 0.3048
}

/// Convert feet per minute to metres per second.
pub(crate) fn fpm_to_ms(fpm: f64) -> f64 {
    fpm * 0.005_08
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorder_writes_numbered_raw_frames() {
        let dir = std::env::temp_dir().join(format!("overflight-recorder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut recorder = Recorder::new(&dir);
        recorder.record("{\"frame\":1}").unwrap();
        recorder.record("{\"frame\":2}").unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.join("frame_0000.json")).unwrap(),
            "{\"frame\":1}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("frame_0001.json")).unwrap(),
            "{\"frame\":2}"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
