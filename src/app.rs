//! Application state: the set of tracked aircraft and the toggles that affect
//! how they are drawn.

use crate::providers::{Aircraft, Query};
use crate::track::Track;

/// Everything the UI needs to draw a frame.
#[derive(Debug)]
pub struct App {
    pub query: Query,
    pub source: String,
    /// Sky orientation (east on the left) or map orientation (east on the right).
    pub sky_orientation: bool,
    pub show_callsigns: bool,
    pub show_trails: bool,
    pub tracks: Vec<Track>,
    /// Seconds at the last successful update.
    pub last_update_s: Option<f64>,
    /// Message from the last failed fetch, if any.
    pub last_error: Option<String>,
    /// Seconds since the app started.
    pub now_s: f64,
}

impl App {
    /// Create an empty app.
    #[must_use]
    pub fn new(query: Query, source: impl Into<String>) -> Self {
        Self {
            query,
            source: source.into(),
            sky_orientation: true,
            show_callsigns: true,
            show_trails: true,
            tracks: Vec::new(),
            last_update_s: None,
            last_error: None,
            now_s: 0.0,
        }
    }

    /// Merge a fresh batch of observations into the tracked set.
    pub fn apply(&mut self, aircraft: &[Aircraft], now_s: f64) {
        let observer = self.query.observer();
        for observation in aircraft {
            if let Some(track) = self
                .tracks
                .iter_mut()
                .find(|track| track.id == observation.id)
            {
                track.apply(observation, observer, now_s);
            } else {
                self.tracks.push(Track::new(observation, observer, now_s));
            }
        }
        self.last_update_s = Some(now_s);
        self.last_error = None;
    }

    /// Record a fetch failure to show in the status line.
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.last_error = Some(message.into());
    }

    /// Advance every track and drop the ones that have gone quiet.
    pub fn update(&mut self, now_s: f64) {
        self.now_s = now_s;
        for track in &mut self.tracks {
            track.update(now_s);
        }
        self.tracks.retain(|track| !track.stale(now_s));
    }

    /// Number of aircraft currently tracked.
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.tracks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query() -> Query {
        Query {
            lat: 52.52,
            lon: 13.40,
            radius_km: 80.0,
        }
    }

    fn observation(id: &str, lat: f64, lon: f64) -> Aircraft {
        Aircraft {
            id: id.to_string(),
            callsign: Some(id.to_uppercase()),
            registration: None,
            type_code: None,
            lat,
            lon,
            alt_m: Some(10_000.0),
            on_ground: false,
            ground_speed_ms: Some(200.0),
            track_deg: Some(90.0),
            vertical_rate_ms: Some(0.0),
            position_age_s: 0.0,
        }
    }

    #[test]
    fn merges_updates_and_drops_stale_aircraft() {
        let mut app = App::new(query(), "test");
        app.apply(&[observation("a", 52.6, 13.5)], 0.0);
        assert_eq!(app.live_count(), 1);
        assert_eq!(app.last_update_s, Some(0.0));

        // The same id updates the existing track, not a new one.
        app.apply(&[observation("a", 52.61, 13.51)], 5.0);
        assert_eq!(app.live_count(), 1);
        assert_eq!(app.tracks[0].last_seen(), 5.0);

        // After a minute with no observation the track is dropped.
        app.update(40.0);
        assert_eq!(app.live_count(), 1);
        app.update(70.0);
        assert_eq!(app.live_count(), 0);
    }

    #[test]
    fn an_error_is_cleared_by_the_next_success() {
        let mut app = App::new(query(), "test");
        app.set_error("boom");
        assert_eq!(app.last_error.as_deref(), Some("boom"));
        app.apply(&[], 1.0);
        assert!(app.last_error.is_none());
    }
}
