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

use crate::providers::{Aircraft, Provider, Query, RateLimited};

/// The wall-clock time `delay` from now, in UTC, as `HH:MM`.
fn clock_after(delay: Duration) -> String {
    let when = std::time::SystemTime::now() + delay;
    let seconds = when
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or(0);
    let utc = crate::sun::Utc::from_unix_seconds(seconds);
    format!("{:02}:{:02} UTC", utc.hour, utc.minute)
}

/// Never wait longer than this between retries.
const MAX_BACKOFF: Duration = Duration::from_secs(120);
/// Sleep in small steps so shutdown is prompt.
const SLEEP_STEP: Duration = Duration::from_millis(100);
/// How long `stop` waits for the thread before detaching it. The thread may be
/// inside a blocking HTTP request, so a clean join could take many seconds.
const STOP_GRACE: Duration = Duration::from_millis(200);

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

    /// Ask the thread to stop.
    ///
    /// We wait at most [`STOP_GRACE`] for the thread to notice and exit, then
    /// detach it. The thread may be inside a blocking HTTP request, and we must
    /// not keep the process alive after the terminal is restored. It holds
    /// nothing that needs flushing.
    pub fn stop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let deadline = std::time::Instant::now() + STOP_GRACE;
            while !handle.is_finished() && std::time::Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            // Dropping the handle detaches the thread if it is still running.
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
        let mut rate_limited: Option<Duration> = None;
        let failed = match provider.fetch(query) {
            Ok(aircraft) => {
                if sender.send(FetchEvent::Aircraft(aircraft)).is_err() {
                    break;
                }
                false
            }
            Err(error) => {
                let message = if let Some(limited) = error.downcast_ref::<RateLimited>() {
                    rate_limited = Some(limited.retry_after);
                    format!(
                        "{} · next try {}",
                        limited.reason,
                        clock_after(limited.retry_after)
                    )
                } else {
                    error.to_string()
                };
                if sender.send(FetchEvent::Error(message)).is_err() {
                    break;
                }
                true
            }
        };

        let wait = if let Some(retry_after) = rate_limited {
            // The provider told us exactly how long to wait; honour it and
            // reset the error backoff.
            backoff = Duration::ZERO;
            retry_after
        } else if failed {
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
    use crate::providers::RateLimited;
    use crate::providers::fixture::FixtureProvider;
    use std::sync::atomic::AtomicU64;

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
            alt_m: 0.0,
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

    /// A provider whose fetch blocks, like a slow HTTP request.
    struct BlockingProvider;

    impl Provider for BlockingProvider {
        fn name(&self) -> &'static str {
            "blocking"
        }
        fn min_interval(&self) -> Duration {
            Duration::from_millis(1)
        }
        fn fetch(&mut self, _query: &Query) -> Result<Vec<Aircraft>> {
            thread::sleep(Duration::from_secs(5));
            Ok(Vec::new())
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
    fn dropping_the_fetcher_is_quick_even_when_the_provider_blocks() {
        let fetcher = Fetcher::spawn(
            Box::new(BlockingProvider),
            query(),
            Duration::from_millis(1),
        );
        // Give the thread time to enter the blocking fetch.
        thread::sleep(Duration::from_millis(50));

        let started = std::time::Instant::now();
        drop(fetcher);
        let elapsed = started.elapsed();
        assert!(
            elapsed < Duration::from_millis(500),
            "dropping the fetcher took {elapsed:?}"
        );
    }

    /// A provider that answers the first call with a rate-limit error.
    struct RateLimitedProvider {
        calls: Arc<AtomicU64>,
    }

    impl Provider for RateLimitedProvider {
        fn name(&self) -> &'static str {
            "limited"
        }
        fn min_interval(&self) -> Duration {
            Duration::from_millis(1)
        }
        fn fetch(&mut self, _query: &Query) -> Result<Vec<Aircraft>> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(RateLimited {
                    retry_after: Duration::from_millis(300),
                    reason: "quota used up",
                }
                .into())
            } else {
                Ok(Vec::new())
            }
        }
    }

    #[test]
    fn a_rate_limited_provider_waits_for_the_retry_time() {
        let calls = Arc::new(AtomicU64::new(0));
        let mut fetcher = Fetcher::spawn(
            Box::new(RateLimitedProvider {
                calls: Arc::clone(&calls),
            }),
            query(),
            Duration::from_millis(1),
        );

        match fetcher.events.recv_timeout(Duration::from_secs(1)) {
            Ok(FetchEvent::Error(message)) => {
                assert!(message.contains("quota used up"), "{message}");
                assert!(message.contains("next try"), "{message}");
            }
            other => panic!("unexpected event: {other:?}"),
        }

        // It must not call the provider again before the retry time.
        thread::sleep(Duration::from_millis(120));
        assert_eq!(calls.load(Ordering::SeqCst), 1, "called again too soon");

        // After the retry time it does.
        thread::sleep(Duration::from_millis(400));
        assert!(calls.load(Ordering::SeqCst) >= 2, "not retried");

        fetcher.stop();
    }

    #[test]
    fn sleeping_stops_early_when_asked() {
        let shutdown = AtomicBool::new(true);
        assert!(!sleep_interruptible(Duration::from_secs(10), &shutdown));
    }
}
