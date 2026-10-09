//! adsb.lol provider.
//!
//! `GET https://api.adsb.lol/v2/point/{lat}/{lon}/{radius_nm}` returns the same
//! readsb-style JSON as airplanes.live, with an `ac` array, and is open to
//! everyone. It asks for at most one request per second, so overflight polls
//! every five seconds by default.
//!
//! This is the default source. Verified live on 2026-10-09: `--dump` from a
//! home connection returned aircraft that moved as expected between polls.

use std::time::Duration;

use anyhow::Result;
use reqwest::blocking::Client;

use super::{Aircraft, Provider, Query, Recorder, point};

const BASE_URL: &str = "https://api.adsb.lol";

/// The adsb.lol point-query provider.
pub struct AdsbLol {
    client: Client,
    base_url: String,
    recorder: Option<Recorder>,
}

impl AdsbLol {
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

impl Provider for AdsbLol {
    fn name(&self) -> &'static str {
        "adsb.lol"
    }

    fn min_interval(&self) -> Duration {
        point::MIN_INTERVAL
    }

    fn fetch(&mut self, query: &Query) -> Result<Vec<Aircraft>> {
        let url = point::point_url(&self.base_url, query);
        point::fetch(
            &self.client,
            &url,
            "adsb.lol",
            "adsb.lol rate limited",
            &mut self.recorder,
        )
    }
}
