//! Application state: the set of tracked aircraft and the toggles that affect
//! how they are drawn.

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::config::Units;
use crate::geo::{self, GeoPoint};
use crate::providers::{Aircraft, Query};
use crate::satellite::{self, Satellite, SatellitePosition};
use crate::sky::{self, BodyPosition};
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
    /// Draw the side-on horizon view instead of the overhead circle.
    pub horizon: bool,
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
    /// Moon and bright planet positions.
    pub bodies: Vec<BodyPosition>,
    /// Satellite element sets, from TLEs.
    pub satellites: Vec<Satellite>,
    /// Satellite positions for the current time.
    pub satellite_positions: Vec<SatellitePosition>,
    /// Fixed star field for the night sky.
    pub stars: Vec<Star>,
    /// Id of the selected aircraft, if any.
    pub selected: Option<String>,
    /// Latest "unusual aircraft" notification and when it arrived.
    pub notification: Option<(String, f64)>,
    /// Preferred display units.
    pub units: Units,
    /// Ignore aircraft whose elevation is below this, degrees.
    pub min_elevation_deg: f64,
}

impl App {
    /// Create an empty app.
    #[must_use]
    pub fn new(query: Query, source: impl Into<String>) -> Self {
        Self {
            query,
            source: source.into(),
            sky_orientation: true,
            horizon: false,
            show_callsigns: true,
            show_trails: true,
            tracks: Vec::new(),
            last_update_s: None,
            last_error: None,
            now_s: 0.0,
            utc_s: 0.0,
            sun_elevation_deg: 90.0,
            bodies: Vec::new(),
            satellites: Vec::new(),
            satellite_positions: Vec::new(),
            stars: generate_stars(),
            selected: None,
            notification: None,
            units: Units::Metric,
            min_elevation_deg: 0.0,
        }
    }

    /// Merge a fresh batch of observations into the tracked set, ignoring any
    /// below the minimum elevation.
    pub fn apply(&mut self, aircraft: &[Aircraft], now_s: f64) {
        let observer = self.query.observer();
        for observation in aircraft {
            if self.elevation_deg(observation) < self.min_elevation_deg {
                continue;
            }
            if let Some(track) = self
                .tracks
                .iter_mut()
                .find(|track| track.id == observation.id)
            {
                track.apply(observation, observer, now_s);
            } else {
                let track = Track::new(observation, observer, now_s);
                if let Some(reason) = track.unusual {
                    self.notification = Some((format!("{} · {reason}", track.label()), now_s));
                }
                self.tracks.push(track);
            }
        }
        self.last_update_s = Some(now_s);
        self.last_error = None;
    }

    fn elevation_deg(&self, observation: &Aircraft) -> f64 {
        let target = GeoPoint::new(
            observation.lat,
            observation.lon,
            observation.alt_m.unwrap_or(0.0),
        );
        geo::az_el(self.query.observer(), target).elevation_deg
    }

    /// Record a fetch failure to show in the status line.
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.last_error = Some(message.into());
    }

    /// Replace the satellite element sets, e.g. after a TLE refresh.
    pub fn set_satellites(&mut self, satellites: Vec<Satellite>) {
        self.satellites = satellites;
    }

    /// Advance every track, recompute the sun, and drop tracks that have gone
    /// quiet or fallen below the minimum elevation. `now_s` is seconds since
    /// start; `utc_s` is the wall-clock time.
    pub fn update(&mut self, now_s: f64, utc_s: f64) {
        self.now_s = now_s;
        self.utc_s = utc_s;
        let time = Utc::from_unix_seconds(utc_s as i64);
        self.sun_elevation_deg = sun::solar_elevation_deg(self.query.lat, self.query.lon, time);
        self.bodies = sky::positions(self.query.lat, self.query.lon, time);

        let observer = self.query.observer();
        self.satellite_positions = match satellite::datetime(utc_s as i64) {
            Some(time) => self
                .satellites
                .iter()
                .filter_map(|satellite| satellite.position(time, observer))
                .collect(),
            None => Vec::new(),
        };
        for track in &mut self.tracks {
            track.update(now_s);
        }
        let min_elevation = self.min_elevation_deg;
        self.tracks
            .retain(|track| !track.stale(now_s) && track.az_el().1 >= min_elevation);
    }

    /// Number of aircraft currently tracked.
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.tracks.len()
    }

    /// Select the next aircraft, nearest first, cycling around.
    pub fn select_next(&mut self) {
        if self.tracks.is_empty() {
            self.selected = None;
            return;
        }
        let mut order: Vec<usize> = (0..self.tracks.len()).collect();
        order.sort_by(|&a, &b| {
            self.tracks[a]
                .az_el()
                .2
                .partial_cmp(&self.tracks[b].az_el().2)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let current = self
            .selected
            .as_ref()
            .and_then(|id| order.iter().position(|&index| &self.tracks[index].id == id));
        let next = current.map_or(0, |position| (position + 1) % order.len());
        self.selected = Some(self.tracks[order[next]].id.clone());
    }

    /// The currently selected aircraft, if it is still tracked.
    #[must_use]
    pub fn selected_track(&self) -> Option<&Track> {
        let id = self.selected.as_deref()?;
        self.tracks.iter().find(|track| track.id == id)
    }

    /// The active "unusual aircraft" notification, if one is still fresh.
    #[must_use]
    pub fn active_notification(&self) -> Option<&str> {
        const NOTIFICATION_SECONDS: f64 = 8.0;
        self.notification.as_ref().and_then(|(message, at)| {
            (self.now_s - at < NOTIFICATION_SECONDS).then_some(message.as_str())
        })
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
            ..Aircraft::default()
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
    fn min_elevation_filters_low_aircraft() {
        let mut app = App::new(query(), "test");
        app.min_elevation_deg = 80.0;
        // Directly overhead passes; a distant aircraft is low on the horizon.
        app.apply(
            &[
                observation("high", 52.52, 13.40),
                observation("low", 52.9, 13.9),
            ],
            0.0,
        );
        assert_eq!(app.live_count(), 1);
        assert_eq!(app.tracks[0].id, "high");
    }

    #[test]
    fn an_unusual_aircraft_raises_a_notification() {
        let mut app = App::new(query(), "test");
        let mut observation = observation("mil", 52.6, 13.5);
        observation.military = true;
        app.apply(&[observation], 0.0);
        assert!(app.active_notification().is_some());

        app.update(20.0, 0.0);
        assert!(app.active_notification().is_none());
    }

    #[test]
    fn selection_cycles_nearest_first() {
        let mut app = App::new(query(), "test");
        app.apply(
            &[
                observation("far", 52.9, 13.9),
                observation("near", 52.53, 13.41),
            ],
            0.0,
        );
        app.select_next();
        assert_eq!(app.selected.as_deref(), Some("near"));
        app.select_next();
        assert_eq!(app.selected.as_deref(), Some("far"));
        app.select_next();
        assert_eq!(app.selected.as_deref(), Some("near"));
        assert!(app.selected_track().is_some());
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
