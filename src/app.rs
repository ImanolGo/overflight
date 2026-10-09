//! Application state: the set of tracked aircraft and the toggles that affect
//! how they are drawn.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::providers::{Aircraft, Query};
use crate::sun::{self, Utc};
use crate::track::Track;

/// Seed for the star field, so it is the same every run and never flickers.
const STAR_SEED: u64 = 0x5EED_0F1E_2D3C_4B5A;
/// How many stars to scatter in the night sky.
const STAR_COUNT: usize = 140;

/// A fixed star, in disc coordinates with the horizon at radius one.
#[derive(Debug, Clone, Copy)]
pub struct Star {
    /// East component, in horizon radii.
    pub east: f64,
    /// North component, in horizon radii.
    pub north: f64,
    /// Relative brightness, `0..1`.
    pub brightness: f64,
}

fn generate_stars() -> Vec<Star> {
    let mut rng = StdRng::seed_from_u64(STAR_SEED);
    (0..STAR_COUNT)
        .map(|_| {
            // sqrt keeps the scatter uniform across the disc, not clumped.
            let radius = rng.random_range(0.0_f64..1.0).sqrt() * 0.98;
            let angle = rng.random_range(0.0..std::f64::consts::TAU);
            Star {
                east: radius * angle.sin(),
                north: radius * angle.cos(),
                brightness: rng.random_range(0.35..1.0),
            }
        })
        .collect()
}

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
    /// Current time as Unix seconds.
    pub utc_s: f64,
    /// Sun elevation above the horizon for the observer, degrees.
    pub sun_elevation_deg: f64,
    /// Fixed star field for the night sky.
    pub stars: Vec<Star>,
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
            utc_s: 0.0,
            sun_elevation_deg: 90.0,
            stars: generate_stars(),
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

    /// Advance every track, recompute the sun, and drop tracks that have gone
    /// quiet. `now_s` is seconds since start; `utc_s` is the wall-clock time.
    pub fn update(&mut self, now_s: f64, utc_s: f64) {
        self.now_s = now_s;
        self.utc_s = utc_s;
        self.sun_elevation_deg = sun::solar_elevation_deg(
            self.query.lat,
            self.query.lon,
            Utc::from_unix_seconds(utc_s as i64),
        );
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
        app.update(40.0, 0.0);
        assert_eq!(app.live_count(), 1);
        app.update(70.0, 0.0);
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

    #[test]
    fn stars_are_fixed_inside_the_disc() {
        let app = App::new(query(), "test");
        assert_eq!(app.stars.len(), STAR_COUNT);
        for star in &app.stars {
            assert!(star.east.hypot(star.north) <= 1.0);
            assert!((0.35..=1.0).contains(&star.brightness));
        }
        let again = App::new(query(), "test");
        let first: Vec<f64> = app.stars.iter().map(|star| star.east).collect();
        let second: Vec<f64> = again.stars.iter().map(|star| star.east).collect();
        assert_eq!(first, second, "star field must be deterministic");
    }

    #[test]
    fn sun_elevation_is_recomputed_from_the_time() {
        let mut app = App::new(query(), "test");
        let noon = sun::parse_rfc3339_seconds("2024-06-20T11:08:00Z").unwrap() as f64;
        app.update(0.0, noon);
        assert!(app.sun_elevation_deg > 0.0, "expected daylight");

        let midnight = sun::parse_rfc3339_seconds("2024-12-21T00:00:00Z").unwrap() as f64;
        app.update(0.0, midnight);
        assert!(app.sun_elevation_deg < -12.0, "expected night");
    }
}
