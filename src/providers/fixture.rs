//! Fixture provider: replays recorded responses with no network access.
//!
//! Used by `--demo` and by tests. The bundled fixture is embedded in the
//! binary so `--demo` keeps working after `cargo install`.

use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;

use super::{Aircraft, Provider, Query, readsb};

/// Recorded responses, captured with `--record` and assembled into one file.
#[derive(Debug, Deserialize)]
struct FixtureDoc {
    #[serde(default)]
    source: String,
    observer: ObserverDoc,
    #[serde(default = "default_interval_s")]
    interval_s: f64,
    frames: Vec<FrameDoc>,
}

#[derive(Debug, Deserialize)]
struct ObserverDoc {
    lat: f64,
    lon: f64,
    radius_km: f64,
}

#[derive(Debug, Deserialize)]
struct FrameDoc {
    response: Value,
}

fn default_interval_s() -> f64 {
    5.0
}

/// A provider that replays a recorded sequence of responses.
pub struct FixtureProvider {
    source: String,
    observer: Query,
    interval: Duration,
    frames: Vec<Value>,
    next: usize,
}

impl FixtureProvider {
    /// Parse a fixture document.
    pub fn from_json_str(json: &str) -> Result<Self> {
        let doc: FixtureDoc = serde_json::from_str(json).context("parsing fixture")?;
        if doc.frames.is_empty() {
            bail!("fixture contains no frames");
        }
        Ok(Self {
            source: doc.source,
            observer: Query {
                lat: doc.observer.lat,
                lon: doc.observer.lon,
                radius_km: doc.observer.radius_km,
            },
            interval: Duration::from_secs_f64(doc.interval_s.max(0.001)),
            frames: doc.frames.into_iter().map(|frame| frame.response).collect(),
            next: 0,
        })
    }

    /// The fixture bundled with the binary, used by `--demo`.
    pub fn embedded() -> Result<Self> {
        Self::from_json_str(include_str!("../../fixtures/demo.json"))
    }

    /// The observer location the fixture was recorded around.
    #[must_use]
    pub const fn query(&self) -> Query {
        self.observer
    }

    /// The source the fixture was recorded from.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }
}

impl Provider for FixtureProvider {
    fn name(&self) -> &'static str {
        "demo"
    }

    fn min_interval(&self) -> Duration {
        self.interval
    }

    fn fetch(&mut self, _query: &Query) -> Result<Vec<Aircraft>> {
        let frame = &self.frames[self.next % self.frames.len()];
        self.next += 1;
        readsb::parse_response(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{
        "source": "test",
        "observer": { "lat": 52.52, "lon": 13.40, "radius_km": 80.0 },
        "interval_s": 5.0,
        "frames": [
            { "response": { "ac": [
                { "hex": "aaa111", "flight": "ONE1", "lat": 52.6, "lon": 13.5, "alt_geom": 30000 }
            ] } },
            { "response": { "ac": [
                { "hex": "aaa111", "flight": "ONE1", "lat": 52.7, "lon": 13.6, "alt_geom": 31000 },
                { "hex": "bbb222", "flight": "TWO2", "lat": 52.4, "lon": 13.3, "alt_geom": 20000 }
            ] } }
        ]
    }"#;

    #[test]
    fn replays_frames_in_order_and_loops() {
        let mut provider = FixtureProvider::from_json_str(FIXTURE).unwrap();
        let query = provider.query();
        assert_eq!(provider.name(), "demo");
        assert_eq!(provider.min_interval(), Duration::from_secs(5));
        assert_eq!(query.lat, 52.52);
        assert_eq!(query.radius_km, 80.0);

        let first = provider.fetch(&query).unwrap();
        assert_eq!(first.len(), 1);
        let second = provider.fetch(&query).unwrap();
        assert_eq!(second.len(), 2);
        // Loops back to the beginning.
        let third = provider.fetch(&query).unwrap();
        assert_eq!(third.len(), 1);
        assert_eq!(third[0].id, "aaa111");
    }

    #[test]
    fn rejects_an_empty_fixture() {
        let empty = r#"{ "observer": { "lat": 0.0, "lon": 0.0, "radius_km": 1.0 }, "frames": [] }"#;
        assert!(FixtureProvider::from_json_str(empty).is_err());
    }

    #[test]
    fn embedded_fixture_parses() {
        // The bundled demo must exist and parse; this is what --demo uses.
        let provider = FixtureProvider::embedded().unwrap();
        assert!(provider.min_interval() >= Duration::from_secs(1));
    }
}
