//! Per-aircraft state: dead reckoning, easing onto fresh observations, trails.
//!
//! Aircraft are tracked in the observer's local East-North-Up frame. The
//! observer is fixed, so dead reckoning is just "anchor position plus velocity
//! times elapsed": the plane's ground speed and track give the horizontal part
//! and its vertical rate the vertical part. When a fresh position arrives we
//! ease from the old prediction to the new one instead of snapping, so a new
//! poll never makes a plane jump.

use std::collections::VecDeque;

use crate::geo::{self, GeoPoint};
use crate::providers::{Aircraft, AircraftKind};

/// Drop an aircraft this many seconds after its last observation.
pub const DROP_AFTER_S: f64 = 60.0;
/// Keep this many seconds of trail.
pub const TRAIL_SECONDS: f64 = 60.0;
/// Ease onto a fresh position over this many seconds.
const EASE_SECONDS: f64 = 1.0;
/// How often to record a trail point.
const TRAIL_SAMPLE_S: f64 = 0.5;
/// Ease the last this many seconds of an aircraft's life, so it fades out.
const FADE_SECONDS: f64 = 5.0;

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], factor: f64) -> [f64; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// Smoothstep, so easing starts and ends gently.
fn smoothstep(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A single aircraft being tracked over time.
#[derive(Debug, Clone)]
pub struct Track {
    pub id: String,
    pub callsign: Option<String>,
    pub registration: Option<String>,
    pub type_code: Option<String>,
    /// Aircraft kind from the ADS-B emitter category.
    pub kind: AircraftKind,
    /// Why this aircraft is unusual, if it is.
    pub unusual: Option<&'static str>,
    pub on_ground: bool,
    pub alt_m: Option<f64>,
    pub track_deg: Option<f64>,
    pub ground_speed_ms: Option<f64>,
    pub vertical_rate_ms: Option<f64>,

    anchor: [f64; 3],
    anchor_time_s: f64,
    velocity: [f64; 3],
    render: [f64; 3],
    ease_from: [f64; 3],
    ease_start_s: f64,
    last_seen_s: f64,
    trail: VecDeque<(f64, [f64; 3])>,
    last_trail_s: f64,
}

impl Track {
    /// Start tracking a fresh observation.
    #[must_use]
    pub fn new(observation: &Aircraft, observer: GeoPoint, now_s: f64) -> Self {
        let velocity = velocity_from(observation);
        let anchor = observation_enu(observation, observer, velocity);
        Self {
            id: observation.id.clone(),
            callsign: observation.callsign.clone(),
            registration: observation.registration.clone(),
            type_code: observation.type_code.clone(),
            kind: observation.kind,
            unusual: observation.unusual_reason(),
            on_ground: observation.on_ground,
            alt_m: observation.alt_m,
            track_deg: observation.track_deg,
            ground_speed_ms: observation.ground_speed_ms,
            vertical_rate_ms: observation.vertical_rate_ms,
            anchor,
            anchor_time_s: now_s,
            velocity,
            render: anchor,
            ease_from: anchor,
            // Start already settled, so the first frame does not ease.
            ease_start_s: now_s - EASE_SECONDS,
            last_seen_s: now_s,
            trail: VecDeque::new(),
            last_trail_s: now_s,
        }
    }

    /// Apply a fresh observation, easing from the current prediction.
    pub fn apply(&mut self, observation: &Aircraft, observer: GeoPoint, now_s: f64) {
        let velocity = velocity_from(observation);
        let anchor = observation_enu(observation, observer, velocity);

        self.ease_from = self.render;
        self.ease_start_s = now_s;
        self.anchor = anchor;
        self.anchor_time_s = now_s;
        self.velocity = velocity;

        self.callsign = observation.callsign.clone();
        self.registration = observation.registration.clone();
        self.type_code = observation.type_code.clone();
        self.kind = observation.kind;
        self.unusual = observation.unusual_reason();
        self.on_ground = observation.on_ground;
        self.alt_m = observation.alt_m;
        self.track_deg = observation.track_deg;
        self.ground_speed_ms = observation.ground_speed_ms;
        self.vertical_rate_ms = observation.vertical_rate_ms;
        self.last_seen_s = now_s;
    }

    /// Advance to `now_s`: ease, dead-reckon, and record a trail point.
    pub fn update(&mut self, now_s: f64) {
        let target = self.predicted(now_s);
        let progress = (now_s - self.ease_start_s) / EASE_SECONDS;
        self.render = if progress >= 1.0 {
            target
        } else {
            lerp(self.ease_from, target, smoothstep(progress))
        };

        while self
            .trail
            .front()
            .is_some_and(|(time, _)| now_s - time > TRAIL_SECONDS)
        {
            self.trail.pop_front();
        }
        if now_s - self.last_trail_s >= TRAIL_SAMPLE_S {
            self.trail.push_back((now_s, self.render));
            self.last_trail_s = now_s;
        }
    }

    /// Dead-reckoned position at `now_s`.
    fn predicted(&self, now_s: f64) -> [f64; 3] {
        add(
            self.anchor,
            scale(self.velocity, now_s - self.anchor_time_s),
        )
    }

    /// Current displayed ENU position, metres.
    #[must_use]
    pub const fn enu(&self) -> [f64; 3] {
        self.render
    }

    /// When this aircraft was last observed, seconds since app start.
    #[must_use]
    pub const fn last_seen(&self) -> f64 {
        self.last_seen_s
    }

    /// Whether this aircraft has gone quiet long enough to drop.
    #[must_use]
    pub fn stale(&self, now_s: f64) -> bool {
        now_s - self.last_seen_s >= DROP_AFTER_S
    }

    /// Azimuth, elevation and slant range of the displayed position.
    #[must_use]
    pub fn az_el(&self) -> (f64, f64, f64) {
        let [east, north, up] = self.render;
        let ground = east.hypot(north);
        (
            east.atan2(north).to_degrees().rem_euclid(360.0),
            up.atan2(ground).to_degrees(),
            (east * east + north * north + up * up).sqrt(),
        )
    }

    /// Trail points and their age, oldest first.
    pub fn trail(&self, now_s: f64) -> impl Iterator<Item = ([f64; 3], f64)> + '_ {
        self.trail
            .iter()
            .map(move |(time, enu)| (*enu, (now_s - time).max(0.0)))
    }

    /// Opacity in `0..=1`, fading out over the last few seconds before dropping.
    #[must_use]
    pub fn alpha(&self, now_s: f64) -> f64 {
        let age = now_s - self.last_seen_s;
        ((DROP_AFTER_S - age) / FADE_SECONDS).clamp(0.0, 1.0)
    }

    /// The label to show: callsign, then registration, then id.
    #[must_use]
    pub fn label(&self) -> &str {
        self.callsign
            .as_deref()
            .or(self.registration.as_deref())
            .unwrap_or(&self.id)
    }
}

/// Velocity in ENU metres per second from an observation's track and rates.
fn velocity_from(observation: &Aircraft) -> [f64; 3] {
    let vertical = observation.vertical_rate_ms.unwrap_or(0.0);
    let Some(track_deg) = observation.track_deg else {
        return [0.0, 0.0, vertical];
    };
    let speed = observation.ground_speed_ms.unwrap_or(0.0);
    let track = track_deg.to_radians();
    [speed * track.sin(), speed * track.cos(), vertical]
}

/// The observation's ENU position, extrapolated to account for its age.
fn observation_enu(observation: &Aircraft, observer: GeoPoint, velocity: [f64; 3]) -> [f64; 3] {
    let target = GeoPoint::new(
        observation.lat,
        observation.lon,
        observation.alt_m.unwrap_or(0.0),
    );
    let enu = geo::enu(observer, target);
    add(enu, scale(velocity, observation.position_age_s))
}

#[cfg(test)]
mod tests {
    use approx::assert_abs_diff_eq;

    use super::*;

    fn observer() -> GeoPoint {
        GeoPoint::new(52.52, 13.40, 0.0)
    }

    fn aircraft(lat: f64, lon: f64, alt_m: f64, track: f64, speed: f64) -> Aircraft {
        Aircraft {
            id: "abc123".to_string(),
            callsign: Some("TEST1".to_string()),
            registration: None,
            type_code: None,
            lat,
            lon,
            alt_m: Some(alt_m),
            on_ground: false,
            ground_speed_ms: Some(speed),
            track_deg: Some(track),
            vertical_rate_ms: Some(0.0),
            position_age_s: 0.0,
            ..Aircraft::default()
        }
    }

    #[test]
    fn dead_reckons_along_the_track() {
        // Due east at 100 m/s from directly north 1 km away.
        let obs = aircraft(52.529, 13.40, 1000.0, 90.0, 100.0);
        let mut track = Track::new(&obs, observer(), 0.0);
        let (_, _, range_0) = track.az_el();

        track.update(10.0);
        let (azimuth, _, range_1) = track.az_el();

        // Moved ~1 km east in 10 s: the range grows and the azimuth swings east.
        assert!(range_1 > range_0 + 200.0, "range {range_0} -> {range_1}");
        assert!(azimuth > 40.0 && azimuth < 50.0, "azimuth {azimuth}");
    }

    #[test]
    fn eases_instead_of_snapping_on_a_new_observation() {
        let obs = aircraft(52.529, 13.40, 1000.0, 90.0, 100.0);
        let mut track = Track::new(&obs, observer(), 0.0);
        track.update(10.0);

        // A corrected observation 500 m north of the prediction.
        let corrected = aircraft(52.534, 13.40, 1000.0, 90.0, 100.0);
        track.apply(&corrected, observer(), 10.0);

        // Immediately after, the displayed position is still near the old one.
        let before = track.enu();
        track.update(10.0);
        let at_ease_start = track.enu();
        assert_abs_diff_eq!(before[1], at_ease_start[1], epsilon = 1e-6);

        // After the ease window it has reached the corrected position.
        track.update(11.0);
        let settled = track.enu();
        let target = Track::new(&corrected, observer(), 10.0);
        let target_enu = target.enu();
        assert_abs_diff_eq!(settled[1], target_enu[1], epsilon = 1e-6);
        assert!((settled[1] - at_ease_start[1]).abs() > 1.0);
    }

    #[test]
    fn records_a_trail_over_time() {
        let obs = aircraft(52.529, 13.40, 1000.0, 90.0, 100.0);
        let mut track = Track::new(&obs, observer(), 0.0);
        track.update(0.0);
        assert_eq!(track.trail(0.0).count(), 0);
        track.update(0.5);
        track.update(1.0);
        assert_eq!(track.trail(1.0).count(), 2);
        // Old points fall off the back after a minute, leaving the new sample.
        track.update(62.0);
        assert_eq!(track.trail(62.0).count(), 1);
    }

    #[test]
    fn alpha_fades_near_the_end_of_life() {
        let obs = aircraft(52.529, 13.40, 1000.0, 90.0, 100.0);
        let track = Track::new(&obs, observer(), 0.0);
        assert_abs_diff_eq!(track.alpha(0.0), 1.0, epsilon = 1e-9);
        assert_abs_diff_eq!(track.alpha(50.0), 1.0, epsilon = 1e-9);
        assert!(track.alpha(57.5) < 1.0 && track.alpha(57.5) > 0.0);
        assert_abs_diff_eq!(track.alpha(60.0), 0.0, epsilon = 1e-9);
    }
}
