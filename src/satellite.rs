//! Satellites from Two-Line Elements, propagated with SGP4.
//!
//! TLEs come from Celestrak by default; a small recorded set is embedded so
//! `--demo` (and the tests) work offline. Positions are TEME from SGP4, rotated
//! into the observer's East-North-Up frame.

use std::path::PathBuf;

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use sgp4::chrono::NaiveDateTime;
use sgp4::{Constants, Elements};

use crate::geo::{self, GeoPoint};

/// The default Celestrak group: crewed stations, including the ISS.
pub const DEFAULT_GROUP: &str = "stations";
/// Ignore anything past this many satellites, to bound the work.
const MAX_SATELLITES: usize = 250;
/// Celestrak updates GP data every two hours; refetch after half a day.
pub const REFRESH_AFTER_S: f64 = 12.0 * 3600.0;
/// After a failure, leave the service alone for a day.
pub const RETRY_AFTER_S: f64 = 24.0 * 3600.0;
/// SGP4 error grows with TLE age; never draw data older than this.
pub const MAX_AGE_S: f64 = 7.0 * 24.0 * 3600.0;

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

/// Fetch the latest TLE text for a Celestrak group.
pub fn fetch_text(client: &reqwest::blocking::Client, group: &str) -> Result<String> {
    let url = format!("https://celestrak.org/NORAD/elements/gp.php?GROUP={group}&FORMAT=tle");
    let text = client
        .get(&url)
        .send()
        .context("Celestrak request")?
        .error_for_status()
        .context("Celestrak returned an error")?
        .text()
        .context("reading Celestrak response")?;
    // A valid response always contains at least one TLE; an empty body usually
    // means we have been throttled.
    if text.trim().is_empty() {
        anyhow::bail!("Celestrak returned an empty response");
    }
    Ok(text)
}

/// When the cached TLEs were fetched and, after an error, when we may retry.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TleMeta {
    pub fetched_at: f64,
    pub retry_after: Option<f64>,
}

/// Whether the cache is due for a refresh at `now`.
#[must_use]
pub fn should_fetch(meta: Option<&TleMeta>, now: f64) -> bool {
    let Some(meta) = meta else {
        return true;
    };
    match meta.retry_after {
        // The last attempt failed: wait for the retry time, regardless of age.
        Some(until) => now >= until,
        // The last attempt succeeded: refresh once the data is old enough.
        None => now - meta.fetched_at >= REFRESH_AFTER_S,
    }
}

/// Whether TLEs fetched at `fetched_at` are too old to draw at `now`.
#[must_use]
pub fn is_stale(fetched_at: f64, now: f64) -> bool {
    now - fetched_at > MAX_AGE_S
}

/// The on-disk TLE cache.
#[derive(Debug)]
pub struct TleCache {
    dir: PathBuf,
}

impl TleCache {
    /// A cache rooted at `dir`.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The cache in the platform cache directory, if one can be found.
    #[must_use]
    pub fn default_location() -> Option<Self> {
        ProjectDirs::from("", "", "overflight").map(|dirs| Self::new(dirs.cache_dir()))
    }

    fn tle_path(&self) -> PathBuf {
        self.dir.join("satellites.tle")
    }

    fn meta_path(&self) -> PathBuf {
        self.dir.join("satellites.meta.json")
    }

    /// The stored TLE text, if any.
    #[must_use]
    pub fn load_tle(&self) -> Option<String> {
        std::fs::read_to_string(self.tle_path()).ok()
    }

    /// The stored metadata, if any.
    #[must_use]
    pub fn load_meta(&self) -> Option<TleMeta> {
        let text = std::fs::read_to_string(self.meta_path()).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Store freshly fetched TLEs.
    pub fn store(&self, raw: &str, fetched_at: f64) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.tle_path(), raw)?;
        self.write_meta(&TleMeta {
            fetched_at,
            retry_after: None,
        })
    }

    /// Record a failure, so we leave the service alone until `until`.
    pub fn store_backoff(&self, until: f64) -> Result<()> {
        let fetched_at = self.load_meta().map_or(until, |meta| meta.fetched_at);
        std::fs::create_dir_all(&self.dir)?;
        self.write_meta(&TleMeta {
            fetched_at,
            retry_after: Some(until),
        })
    }

    fn write_meta(&self, meta: &TleMeta) -> Result<()> {
        std::fs::write(self.meta_path(), serde_json::to_string(meta)?)?;
        Ok(())
    }
}

/// The result of [`refresh`].
#[derive(Debug)]
pub enum Refresh {
    /// New data was fetched and cached.
    Fetched {
        satellites: Vec<Satellite>,
        fetched_at: f64,
    },
    /// The cache was used; it may be old.
    Cached {
        satellites: Vec<Satellite>,
        fetched_at: f64,
    },
    /// Nothing usable.
    Unavailable,
}

impl Refresh {
    /// The satellites and when the underlying TLEs were fetched.
    #[must_use]
    pub fn into_parts(self) -> (Vec<Satellite>, f64) {
        match self {
            Refresh::Fetched {
                satellites,
                fetched_at,
            }
            | Refresh::Cached {
                satellites,
                fetched_at,
            } => (satellites, fetched_at),
            Refresh::Unavailable => (Vec::new(), 0.0),
        }
    }
}

/// Use the cache, refreshing first only when it is due. `fetch` is called at
/// most once and only when a refresh is due.
pub fn refresh<F>(cache: &TleCache, now: f64, fetch: F) -> Refresh
where
    F: FnOnce() -> Result<String>,
{
    let meta = cache.load_meta();
    if should_fetch(meta.as_ref(), now)
        && let Ok(raw) = fetch()
        && let Ok(satellites) = parse(&raw)
    {
        let _ = cache.store(&raw, now);
        return Refresh::Fetched {
            satellites,
            fetched_at: now,
        };
    }
    if should_fetch(meta.as_ref(), now) {
        // The fetch failed; back off for a day.
        let _ = cache.store_backoff(now + RETRY_AFTER_S);
    }

    match cache.load_tle().map(|raw| parse(&raw)) {
        Some(Ok(satellites)) => Refresh::Cached {
            satellites,
            fetched_at: meta.map_or(now, |meta| meta.fetched_at),
        },
        _ => Refresh::Unavailable,
    }
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

    const SAMPLE: &str = include_str!("../fixtures/satellites.tle");

    fn temp_cache(name: &str) -> TleCache {
        let dir =
            std::env::temp_dir().join(format!("overflight-tle-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        TleCache::new(dir)
    }

    #[test]
    fn tle_cache_round_trips() {
        let cache = temp_cache("round");
        assert!(cache.load_tle().is_none());
        assert!(cache.load_meta().is_none());

        cache.store(SAMPLE, 1000.0).unwrap();
        assert_eq!(cache.load_tle().as_deref(), Some(SAMPLE));
        let meta = cache.load_meta().unwrap();
        assert_eq!(meta.fetched_at, 1000.0);
        assert!(meta.retry_after.is_none());

        let _ = std::fs::remove_dir_all(&cache.dir);
    }

    #[test]
    fn should_fetch_respects_freshness_and_backoff() {
        assert!(should_fetch(None, 0.0));
        let fresh = TleMeta {
            fetched_at: 1000.0,
            retry_after: None,
        };
        assert!(!should_fetch(Some(&fresh), 1000.0 + REFRESH_AFTER_S - 1.0));
        assert!(should_fetch(Some(&fresh), 1000.0 + REFRESH_AFTER_S + 1.0));

        let backed_off = TleMeta {
            fetched_at: 0.0,
            retry_after: Some(2000.0),
        };
        assert!(!should_fetch(Some(&backed_off), 1500.0));
        assert!(should_fetch(Some(&backed_off), 2001.0));
    }

    #[test]
    fn ten_launches_only_fetch_once() {
        let cache = temp_cache("ten");
        let mut fetches = 0;
        for launch in 0..10 {
            let now = 1_000_000.0 + f64::from(launch) * 60.0;
            let outcome = refresh(&cache, now, || {
                fetches += 1;
                Ok(SAMPLE.to_string())
            });
            assert!(matches!(
                outcome,
                Refresh::Fetched { .. } | Refresh::Cached { .. }
            ));
        }
        assert_eq!(fetches, 1, "ten launches should make one request");
        let _ = std::fs::remove_dir_all(&cache.dir);
    }

    #[test]
    fn a_failure_backs_off_for_a_day() {
        let cache = temp_cache("backoff");
        let mut fetches = 0;

        let first = refresh(&cache, 1000.0, || {
            fetches += 1;
            anyhow::bail!("network down")
        });
        assert!(matches!(first, Refresh::Unavailable));
        assert_eq!(fetches, 1);

        // A minute later, still within the backoff: no request.
        let second = refresh(&cache, 1060.0, || {
            fetches += 1;
            Ok(SAMPLE.to_string())
        });
        assert!(matches!(second, Refresh::Unavailable));
        assert_eq!(fetches, 1);

        // After the backoff expires it tries again.
        let third = refresh(&cache, 1000.0 + RETRY_AFTER_S + 1.0, || {
            fetches += 1;
            Ok(SAMPLE.to_string())
        });
        assert!(matches!(third, Refresh::Fetched { .. }));
        assert_eq!(fetches, 2);

        let _ = std::fs::remove_dir_all(&cache.dir);
    }

    #[test]
    fn stale_data_is_flagged() {
        assert!(!is_stale(0.0, MAX_AGE_S));
        assert!(is_stale(0.0, MAX_AGE_S + 1.0));
    }
}
