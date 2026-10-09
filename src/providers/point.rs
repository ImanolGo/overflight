//! Shared plumbing for readsb-style point-query APIs.
//!
//! airplanes.live and adsb.lol both serve
//! `GET <base>/v2/point/{lat}/{lon}/{radius_nm}`, where the radius is in
//! nautical miles (250 at most) and the response is readsb JSON with an `ac`
//! array. Both ask for no more than one request a second.

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;

use super::{Aircraft, Query, Recorder, readsb};

/// The largest radius the point APIs accept, in nautical miles.
pub(crate) const MAX_RADIUS_NM: f64 = 250.0;

/// The shortest gap the point APIs allow between requests.
pub(crate) const MIN_INTERVAL: Duration = Duration::from_secs(5);

/// Build the point-query URL, converting kilometres to nautical miles and
/// clamping to the API's 250 nm maximum.
pub(crate) fn point_url(base_url: &str, query: &Query) -> String {
    let radius_nm = (query.radius_km / 1.852).ceil().clamp(1.0, MAX_RADIUS_NM);
    format!(
        "{base_url}/v2/point/{:.4}/{:.4}/{radius_nm:.0}",
        query.lat, query.lon
    )
}

/// Fetch a point-query response and parse it into aircraft.
///
/// `source` names the API in error messages; `rate_reason` is the short reason
/// attached to a [`super::RateLimited`] error, which must be a `'static` string.
pub(crate) fn fetch(
    client: &Client,
    url: &str,
    source: &str,
    rate_reason: &'static str,
    recorder: &mut Option<Recorder>,
) -> Result<Vec<Aircraft>> {
    let response = client
        .get(url)
        .send()
        .with_context(|| format!("{source} request"))?;
    if let Some(limited) = super::rate_limit_from(&response, rate_reason, Duration::from_secs(60)) {
        return Err(limited.into());
    }
    let raw = response
        .error_for_status()
        .with_context(|| format!("{source} returned an error"))?
        .text()
        .with_context(|| format!("reading {source} response"))?;

    if let Some(recorder) = recorder {
        recorder.record(&raw)?;
    }

    let value: serde_json::Value =
        serde_json::from_str(&raw).with_context(|| format!("{source} response was not JSON"))?;
    readsb::parse_response(&value)
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
            alt_m: 0.0,
        };
        // 37 km / 1.852 = 19.98 nm, rounded up.
        assert_eq!(
            point_url("https://api.adsb.lol", &query),
            "https://api.adsb.lol/v2/point/51.4703/-0.4543/20"
        );
    }

    #[test]
    fn clamps_the_radius_to_the_api_maximum() {
        let query = Query {
            lat: 0.0,
            lon: 0.0,
            radius_km: 1000.0,
            alt_m: 0.0,
        };
        assert!(point_url("http://x", &query).ends_with("/250"));
    }
}
