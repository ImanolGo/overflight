//! Callsign route lookup, via [adsbdb](https://www.adsbdb.com/).
//!
//! Results are cached per callsign for a day. Lookups run on a worker thread
//! and failures are silent: a route is a nicety, not essential data.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;

/// How long a cached route (or a cached "unknown") is trusted.
pub const CACHE_TTL_S: f64 = 24.0 * 3600.0;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

/// An origin and destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    pub origin: String,
    pub destination: String,
}

/// Something that can look up a callsign's route.
pub trait RouteSource: Send {
    /// The route for a callsign, or `None` if it is unknown or unavailable.
    fn lookup(&self, callsign: &str) -> Option<Route>;
}

/// The real adsbdb client.
pub struct Adsbdb {
    client: Client,
    base_url: String,
}

impl Adsbdb {
    /// Create a client against the public API.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            client,
            base_url: "https://api.adsbdb.com".to_string(),
        }
    }
}

impl RouteSource for Adsbdb {
    fn lookup(&self, callsign: &str) -> Option<Route> {
        let url = format!("{}/v0/callsign/{}", self.base_url, callsign);
        let response = self.client.get(&url).send().ok()?;
        if !response.status().is_success() {
            return None;
        }
        parse_route(&response.text().ok()?)
    }
}

/// Parse an adsbdb callsign response into a route.
#[must_use]
pub fn parse_route(text: &str) -> Option<Route> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let route = value.get("response")?.get("flightroute")?;
    let code = |key: &str| {
        route
            .get(key)?
            .get("iata_code")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    };
    Some(Route {
        origin: code("origin")?,
        destination: code("destination")?,
    })
}

/// A worker thread plus a per-callsign cache.
#[derive(Debug)]
pub struct Looker {
    requests: Sender<String>,
    results: Receiver<(String, Option<Route>)>,
    cache: HashMap<String, (Option<Route>, f64)>,
    in_flight: HashSet<String>,
}

impl Looker {
    /// Spawn the lookup worker.
    #[must_use]
    pub fn spawn(source: Box<dyn RouteSource>) -> Self {
        let (requests, request_rx) = mpsc::channel::<String>();
        let (result_tx, results) = mpsc::channel();
        std::thread::spawn(move || {
            // `request_rx` ends when the Looker is dropped, so the thread exits.
            while let Ok(callsign) = request_rx.recv() {
                let route = source.lookup(&callsign);
                if result_tx.send((callsign, route)).is_err() {
                    break;
                }
            }
        });
        Self {
            requests,
            results,
            cache: HashMap::new(),
            in_flight: HashSet::new(),
        }
    }

    /// Ask for a callsign's route, unless it is cached or already in flight.
    pub fn request(&mut self, callsign: &str, now: f64) {
        if self.is_cached(callsign, now) || self.in_flight.contains(callsign) {
            return;
        }
        if self.requests.send(callsign.to_string()).is_ok() {
            self.in_flight.insert(callsign.to_string());
        }
    }

    /// Collect any finished lookups.
    pub fn poll(&mut self, now: f64) {
        while let Ok((callsign, route)) = self.results.try_recv() {
            self.in_flight.remove(&callsign);
            self.cache.insert(callsign, (route, now));
        }
    }

    /// The cached route for a callsign, if a fresh one is known.
    #[must_use]
    pub fn get(&self, callsign: &str, now: f64) -> Option<&Route> {
        let (route, at) = self.cache.get(callsign)?;
        if now - at < CACHE_TTL_S {
            route.as_ref()
        } else {
            None
        }
    }

    fn is_cached(&self, callsign: &str, now: f64) -> bool {
        self.cache
            .get(callsign)
            .is_some_and(|(_, at)| now - at < CACHE_TTL_S)
    }
}

/// A blocking HTTP client for route lookups.
pub fn http_client() -> Result<Client> {
    Client::builder()
        .user_agent(crate::providers::USER_AGENT)
        .connect_timeout(Duration::from_secs(5))
        .timeout(REQUEST_TIMEOUT)
        .build()
        .context("building the route HTTP client")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_route() {
        let text = r#"{"response":{"flightroute":{"origin":{"iata_code":"LHR"},
            "destination":{"iata_code":"LIN"}}}}"#;
        assert_eq!(
            parse_route(text),
            Some(Route {
                origin: "LHR".to_string(),
                destination: "LIN".to_string(),
            })
        );
        assert_eq!(parse_route(r#"{"response":"unknown callsign"}"#), None);
    }

    struct FakeSource {
        routes: HashMap<String, Route>,
    }

    impl RouteSource for FakeSource {
        fn lookup(&self, callsign: &str) -> Option<Route> {
            self.routes.get(callsign).cloned()
        }
    }

    fn wait_for_poll(looker: &mut Looker, now: f64) -> Option<Route> {
        for _ in 0..50 {
            looker.poll(now);
            if let Some(route) = looker.get("DLH4AB", now) {
                return Some(route.clone());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }

    #[test]
    fn caches_a_route_for_a_day() {
        let mut routes = HashMap::new();
        routes.insert(
            "DLH4AB".to_string(),
            Route {
                origin: "FRA".to_string(),
                destination: "LHR".to_string(),
            },
        );
        let mut looker = Looker::spawn(Box::new(FakeSource { routes }));

        looker.request("DLH4AB", 0.0);
        let route = wait_for_poll(&mut looker, 1.0).expect("looked up");
        assert_eq!(route.origin, "FRA");

        // Still fresh just before the TTL, expired just after.
        assert!(looker.get("DLH4AB", CACHE_TTL_S - 1.0).is_some());
        assert!(looker.get("DLH4AB", CACHE_TTL_S + 1.0).is_none());
    }

    #[test]
    fn unknown_callsigns_are_cached_too() {
        let mut looker = Looker::spawn(Box::new(FakeSource {
            routes: HashMap::new(),
        }));
        looker.request("NOPE", 0.0);
        for _ in 0..50 {
            looker.poll(1.0);
            if looker.is_cached("NOPE", 1.0) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(looker.is_cached("NOPE", 1.0));
        assert!(looker.get("NOPE", 1.0).is_none());
    }
}
