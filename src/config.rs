//! Config file loading.
//!
//! The config lives at the platform config directory (on Linux,
//! `~/.config/overflight/config.toml`) and supplies defaults for the CLI flags.
//! Command-line flags always win.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::Deserialize;

/// Preferred display units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Units {
    #[default]
    Metric,
    Imperial,
}

impl Units {
    /// The other set of units.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Units::Metric => Units::Imperial,
            Units::Imperial => Units::Metric,
        }
    }

    /// Convert metres to the preferred altitude unit.
    #[must_use]
    pub fn altitude(self, metres: f64) -> (f64, &'static str) {
        match self {
            Units::Metric => (metres, "m"),
            Units::Imperial => (metres * 3.280_84, "ft"),
        }
    }

    /// Convert metres per second to the preferred speed unit.
    #[must_use]
    pub fn speed(self, metres_per_second: f64) -> (f64, &'static str) {
        match self {
            Units::Metric => (metres_per_second * 3.6, "km/h"),
            Units::Imperial => (metres_per_second * 1.943_844, "kn"),
        }
    }

    /// Convert kilometres to the preferred distance unit.
    #[must_use]
    pub fn distance(self, kilometres: f64) -> (f64, &'static str) {
        match self {
            Units::Metric => (kilometres, "km"),
            Units::Imperial => (kilometres * 0.621_371, "mi"),
        }
    }
}

/// Settings read from the config file. Everything is optional; CLI flags and
/// built-in defaults fill the gaps.
#[derive(Debug, Default, Deserialize)]
pub struct FileConfig {
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// Observer height above sea level, in metres.
    pub alt_m: Option<f64>,
    pub radius_km: Option<f64>,
    pub units: Option<Units>,
    pub min_elevation: Option<f64>,
    pub interval: Option<u64>,
    pub source: Option<String>,
    pub url: Option<String>,
    /// Celestrak group for satellites.
    pub tle_group: Option<String>,
    /// Extra rare type codes to highlight.
    pub rare_types: Option<Vec<String>>,
    /// OpenSky client id, if using `source = "opensky"`.
    pub opensky_client_id: Option<String>,
    /// OpenSky client secret, if using `source = "opensky"`.
    pub opensky_client_secret: Option<String>,
}

impl FileConfig {
    /// Load and parse a config file.
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// The default config file location, if a config directory can be found.
    #[must_use]
    pub fn default_path() -> Option<PathBuf> {
        ProjectDirs::from("", "", "overflight").map(|dirs| dirs.config_dir().join("config.toml"))
    }

    /// Load the default config file if it exists.
    pub fn load_default() -> Result<Option<(PathBuf, Self)>> {
        match Self::default_path() {
            Some(path) if path.exists() => {
                let config = Self::load(&path)?;
                Ok(Some((path, config)))
            }
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_config_file() {
        let config: FileConfig = toml::from_str(
            r#"
            lat = 52.52
            lon = 13.40
            radius_km = 80
            units = "imperial"
            min_elevation = 10
            interval = 5
            source = "opensky"
            "#,
        )
        .unwrap();
        assert_eq!(config.lat, Some(52.52));
        assert_eq!(config.units, Some(Units::Imperial));
        assert_eq!(config.min_elevation, Some(10.0));
        assert_eq!(config.interval, Some(5));
        assert_eq!(config.source.as_deref(), Some("opensky"));
    }

    #[test]
    fn an_empty_config_is_valid() {
        let config: FileConfig = toml::from_str("").unwrap();
        assert_eq!(config.lat, None);
        assert_eq!(config.units, None);
    }

    #[test]
    fn units_convert_both_ways() {
        assert_eq!(Units::Metric.toggled(), Units::Imperial);
        assert_eq!(Units::Imperial.toggled(), Units::Metric);
        assert_eq!(Units::Metric.altitude(1000.0), (1000.0, "m"));
        assert!((Units::Imperial.altitude(1000.0).0 - 3280.84).abs() < 0.01);
        assert!((Units::Imperial.speed(100.0).0 - 194.3844).abs() < 0.01);
        assert!((Units::Imperial.distance(100.0).0 - 62.1371).abs() < 0.01);
    }

    #[test]
    fn default_path_points_at_the_config_file() {
        let path = FileConfig::default_path().unwrap();
        assert_eq!(path.file_name().unwrap(), "config.toml");
        assert!(path.to_string_lossy().contains("overflight"));
    }
}
