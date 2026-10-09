//! Background fetch thread.
//!
//! Owns the provider, polls it on an interval no faster than the provider
//! allows, backs off exponentially on errors, and sends results to the UI over
//! a channel. Stops when asked or when the receiver is dropped.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::providers::{Aircraft, Provider, Query};

/// Never wait longer than this between retries.
const MAX_BACKOFF: Duration = Duration::from_secs(120);
/// Sleep in small steps so shutdown is prompt.
const SLEEP_STEP: Duration = Duration::from_millis(100);

/// A message from the fetch thread.
#[derive(Debug)]
pub enum FetchEvent {
    Aircraft(Vec<Aircraft>),
    Error(String),
}

/// Handle to the background fetch thread.
pub struct Fetcher {
    /// Results from the provider, to be drained by the UI.
    pub events: Receiver<FetchEvent>,
    shutdown: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Fetcher {
    /// Spawn the fetch thread.
    ///
    /// `interval` is the desired gap between polls; the provider's own minimum
    /// still applies.
    #[must_use]
    pub fn spawn(provider: Box<dyn Provider>, query: Query, interval: Duration) -> Self {
        let (sender, events) = mpsc::channel();
        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_flag = Arc::clone(&shutdown);
        let handle = thread::spawn(move || run(provider, &query, interval, &sender, &thread_flag));
        Self {
            events,
            shutdown,
            handle: Some(handle),
        }
    }

    /// Ask the thread to stop and wait for it.
    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Fetcher {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(
    mut provider: Box<dyn Provider>,
    query: &Query,
    interval: Duration,
    sender: &Sender<FetchEvent>,
    shutdown: &AtomicBool,
) {
    let base = interval.max(provider.min_interval());
    let mut backoff = Duration::ZERO;

    while !shutdown.load(Ordering::Relaxed) {
        let failed = match provider.fetch(query) {
            Ok(aircraft) => {
                if sender.send(FetchEvent::Aircraft(aircraft)).is_err() {
                    break;
                }
                false
            }
            Err(error) => {
                if sender.send(FetchEvent::Error(error.to_string())).is_err() {
                    break;
                }
                true
            }
        };

        let wait = if failed {
            backoff = if backoff.is_zero() {
                base
            } else {
                (backoff * 2).min(MAX_BACKOFF)
            };
            backoff
        } else {
            backoff = Duration::ZERO;
            base
        };

        if !sleep_interruptible(wait, shutdown) {
            break;
        }
    }
}

/// Sleep for `total`, returning `false` early if asked to shut down.
fn sleep_interruptible(total: Duration, shutdown: &AtomicBool) -> bool {
    let mut slept = Duration::ZERO;
    while slept < total {
        if shutdown.load(Ordering::Relaxed) {
            return false;
        }
        let step = SLEEP_STEP.min(total - slept);
        thread::sleep(step);
        slept += step;
    }
    !shutdown.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use anyhow::{Result, anyhow};

    use super::*;
    use crate::providers::fixture::FixtureProvider;

    const FIXTURE: &str = r#"{
        "observer": { "lat": 52.52, "lon": 13.40, "radius_km": 80.0 },
        "interval_s": 0.001,
        "frames": [
            { "response": { "ac": [
                { "hex": "aaa111", "lat": 52.6, "lon": 13.5, "alt_geom": 30000 }
            ] } }
        ]
    }"#;

    fn query() -> Query {
        Query {
            lat: 52.52,
            lon: 13.40,
            radius_km: 80.0,
        }
    }

    struct FailingProvider;

    impl Provider for FailingProvider {
        fn name(&self) -> &'static str {
            "failing"
        }
        fn min_interval(&self) -> Duration {
            Duration::from_millis(1)
        }
        fn fetch(&mut self, _query: &Query) -> Result<Vec<Aircraft>> {
            Err(anyhow!("no data"))
        }
    }

    #[test]
    fn fetches_repeatedly_then_stops() {
        let provider = Box::new(FixtureProvider::from_json_str(FIXTURE).unwrap());
        let mut fetcher = Fetcher::spawn(provider, query(), Duration::from_millis(1));

        for _ in 0..3 {
            match fetcher.events.recv_timeout(Duration::from_secs(2)) {
                Ok(FetchEvent::Aircraft(aircraft)) => assert_eq!(aircraft.len(), 1),
                other => panic!("unexpected event: {other:?}"),
            }
        }
        fetcher.stop();
    }

    #[test]
    fn reports_errors_from_the_provider() {
        let provider = Box::new(FailingProvider);
        let mut fetcher = Fetcher::spawn(provider, query(), Duration::from_millis(1));
        match fetcher.events.recv_timeout(Duration::from_secs(2)) {
            Ok(FetchEvent::Error(message)) => assert_eq!(message, "no data"),
            other => panic!("unexpected event: {other:?}"),
        }
        fetcher.stop();
    }

    #[test]
    fn sleeping_stops_early_when_asked() {
        let shutdown = AtomicBool::new(true);
        assert!(!sleep_interruptible(Duration::from_secs(10), &shutdown));
    }
}
