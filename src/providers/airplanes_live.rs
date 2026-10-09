//! airplanes.live provider.
//!
//! `GET https://api.airplanes.live/v2/point/{lat}/{lon}/{radius_nm}` returns
//! readsb-style JSON with an `ac` array. The service asks for at most one
//! request per second, so overflight polls every five seconds by default.
//!
//! Note: airplanes.live answered this project's requests with
//! `403 Please contact us...` from the development environment, so the client
//! is written to the documented API but was not exercised live here.

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;

use super::{Aircraft, Provider, Query, Recorder, readsb};

const BASE_URL: &str = "https://api.airplanes.live";
const MAX_RADIUS_NM: f64 = 250.0;

/// The airplanes.live point-query provider.
pub struct AirplanesLive {
    client: Client,
    base_url: String,
    recorder: Option<Recorder>,
}

impl AirplanesLive {
    /// Create the provider against the public API.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            base_url: BASE_URL.to_string(),
            recorder: None,
        }
    }

    /// Also save each raw response with `recorder`.
    #[must_use]
    pub fn with_recorder(mut self, recorder: Recorder) -> Self {
        self.recorder = Some(recorder);
        self
    }
}

impl Provider for AirplanesLive {
    fn name(&self) -> &'static str {
        "airplanes.live"
    }

    fn min_interval(&self) -> Duration {
        Duration::from_secs(5)
    }

    fn fetch(&mut self, query: &Query) -> Result<Vec<Aircraft>> {
        let url = point_url(&self.base_url, query);
        let raw = self
            .client
            .get(&url)
            .send()
            .context("airplanes.live request")?
            .error_for_status()
            .context("airplanes.live returned an error")?
            .text()
            .context("reading airplanes.live response")?;

        if let Some(recorder) = &mut self.recorder {
            recorder.record(&raw)?;
        }

        let value: serde_json::Value =
            serde_json::from_str(&raw).context("airplanes.live response was not JSON")?;
        readsb::parse_response(&value)
    }
}

/// Build the point-query URL, converting kilometres to nautical miles and
/// clamping to the API's 250 nm maximum.
fn point_url(base_url: &str, query: &Query) -> String {
    let radius_nm = (query.radius_km / 1.852).ceil().clamp(1.0, MAX_RADIUS_NM);
    format!(
        "{base_url}/v2/point/{:.4}/{:.4}/{radius_nm:.0}",
        query.lat, query.lon
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_point_url_in_nautical_miles() {
        let query = Query {
            lat: 51.4703,
            lon: -0.4543,
            radius_km: 37.0,
        };
        // 37 km / 1.852 = 19.98 nm, rounded up.
        assert_eq!(
            point_url("https://api.airplanes.live", &query),
            "https://api.airplanes.live/v2/point/51.4703/-0.4543/20"
        );
    }

    #[test]
    fn clamps_the_radius_to_the_api_maximum() {
        let query = Query {
            lat: 0.0,
            lon: 0.0,
            radius_km: 1000.0,
        };
        assert!(point_url("http://x", &query).ends_with("/250"));
    }
}
