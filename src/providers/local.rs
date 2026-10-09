//! Local ADS-B receiver provider.
//!
//! `--source local --url http://host/data/aircraft.json` reads a readsb or
//! dump1090 `aircraft.json`. The file contains everything the receiver hears,
//! so we filter to the query radius ourselves. There is no rate limit, so
//! overflight polls once a second.

use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;

use super::{Aircraft, Provider, Query, Recorder, readsb};

/// A provider backed by a local receiver's `aircraft.json`.
pub struct Local {
    client: Client,
    url: String,
    recorder: Option<Recorder>,
}

impl Local {
    /// Create a provider for the given `aircraft.json` URL.
    #[must_use]
    pub fn new(client: Client, url: impl Into<String>) -> Self {
        Self {
            client,
            url: url.into(),
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

impl Provider for Local {
    fn name(&self) -> &'static str {
        "local"
    }

    fn min_interval(&self) -> Duration {
        Duration::from_secs(1)
    }

    fn fetch(&mut self, query: &Query) -> Result<Vec<Aircraft>> {
        let raw = self
            .client
            .get(&self.url)
            .send()
            .context("local receiver request")?
            .error_for_status()
            .context("local receiver returned an error")?
            .text()
            .context("reading local receiver response")?;

        if let Some(recorder) = &mut self.recorder {
            recorder.record(&raw)?;
        }

        let value: serde_json::Value =
            serde_json::from_str(&raw).context("local receiver response was not JSON")?;
        let aircraft = readsb::parse_response(&value)?;
        Ok(aircraft
            .into_iter()
            .filter(|aircraft| query.contains(aircraft))
            .collect())
    }
}
