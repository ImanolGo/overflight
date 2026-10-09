//! Shared parser for readsb-style JSON.
//!
//! Both airplanes.live (`{"ac": [...]}`) and a local receiver
//! (`{"aircraft": [...]}`) use the same per-aircraft field format, documented
//! in readsb's `README-json.md`. Every field is optional; aircraft without a
//! position are skipped.

use anyhow::{Result, anyhow};
use serde::Deserialize;
use serde_json::Value;

use super::{Aircraft, feet_to_m, fpm_to_ms, knots_to_ms};

/// An altitude field, which readsb encodes as feet or the string `"ground"`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Altitude {
    Feet(f64),
    Text(String),
}

impl Altitude {
    fn metres(&self) -> Option<f64> {
        match self {
            Altitude::Feet(feet) => Some(feet_to_m(*feet)),
            Altitude::Text(_) => None,
        }
    }

    fn is_ground(&self) -> bool {
        matches!(self, Altitude::Text(text) if text == "ground")
    }
}

/// A squawk, which some feeds give as a string of octal digits and others as a
/// number.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Squawk {
    Text(String),
    Number(u32),
}

impl Squawk {
    fn code(&self) -> String {
        match self {
            Squawk::Text(text) => text.clone(),
            Squawk::Number(number) => format!("{number:04}"),
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RawAircraft {
    hex: Option<String>,
    flight: Option<String>,
    r: Option<String>,
    t: Option<String>,
    category: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    alt_geom: Option<Altitude>,
    alt_baro: Option<Altitude>,
    gs: Option<f64>,
    track: Option<f64>,
    geom_rate: Option<f64>,
    baro_rate: Option<f64>,
    seen_pos: Option<f64>,
    squawk: Option<Squawk>,
    emergency: Option<String>,
    #[serde(rename = "dbFlags")]
    db_flags: Option<u32>,
}

impl RawAircraft {
    fn into_aircraft(self) -> Option<Aircraft> {
        let id = self.hex?.to_lowercase();
        if id.is_empty() {
            return None;
        }
        let lat = self.lat?;
        let lon = self.lon?;

        let on_ground = self.alt_baro.as_ref().is_some_and(Altitude::is_ground);
        // alt_baro is pressure altitude (close to height above sea level) and
        // alt_geom is GNSS height above the ellipsoid; at this scale the
        // difference does not matter, so prefer the geometric one.
        let alt_m = self
            .alt_geom
            .as_ref()
            .and_then(Altitude::metres)
            .or_else(|| self.alt_baro.as_ref().and_then(Altitude::metres));

        let callsign = self
            .flight
            .map(|flight| flight.trim().to_string())
            .filter(|flight| !flight.is_empty());

        let emergency = self
            .emergency
            .map(|emergency| emergency.trim().to_string())
            .filter(|emergency| !emergency.is_empty() && emergency != "none");

        Some(Aircraft {
            id,
            callsign,
            registration: self.r,
            type_code: self.t,
            kind: kind_from_category(self.category.as_deref()),
            lat,
            lon,
            alt_m,
            on_ground,
            ground_speed_ms: self.gs.map(knots_to_ms),
            track_deg: self.track,
            vertical_rate_ms: self.geom_rate.or(self.baro_rate).map(fpm_to_ms),
            position_age_s: self.seen_pos.unwrap_or(0.0),
            squawk: self.squawk.map(|squawk| squawk.code()),
            emergency,
            military: self.db_flags.is_some_and(|flags| flags & 1 != 0),
            interesting: self.db_flags.is_some_and(|flags| flags & 2 != 0),
        })
    }
}

/// Map a readsb emitter category (`A0`..`D7`) to a kind.
fn kind_from_category(category: Option<&str>) -> super::AircraftKind {
    use super::AircraftKind;
    match category {
        Some("A7") => AircraftKind::Helicopter,
        Some("B1") => AircraftKind::Glider,
        Some("B2") => AircraftKind::Balloon,
        _ => AircraftKind::Plane,
    }
}

/// Parse a whole readsb-style response (an object with an `ac` or `aircraft`
/// array).
pub fn parse_response(value: &Value) -> Result<Vec<Aircraft>> {
    let array = value
        .get("ac")
        .or_else(|| value.get("aircraft"))
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("response has no 'ac' or 'aircraft' array"))?;
    Ok(parse_aircraft(array))
}

/// Parse an array of readsb aircraft objects, skipping malformed entries and
/// any without a position.
#[must_use]
pub fn parse_aircraft(array: &[Value]) -> Vec<Aircraft> {
    array
        .iter()
        .filter_map(|value| {
            serde_json::from_value::<RawAircraft>(value.clone())
                .ok()
                .and_then(RawAircraft::into_aircraft)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::providers::AircraftKind;

    fn only(json: Value) -> Aircraft {
        let parsed = parse_response(&json).expect("valid response");
        assert_eq!(parsed.len(), 1, "expected exactly one aircraft");
        parsed.into_iter().next().unwrap()
    }

    #[test]
    fn parses_a_complete_aircraft() {
        let aircraft = only(json!({
            "ac": [{
                "hex": "3C675A",
                "flight": "DLH4AB  ",
                "r": "D-AIZZ",
                "t": "A20N",
                "lat": 51.47,
                "lon": -0.45,
                "alt_geom": 36000,
                "alt_baro": 35750,
                "gs": 450.0,
                "track": 271.5,
                "geom_rate": -640,
                "baro_rate": -704,
                "seen_pos": 0.9
            }]
        }));

        assert_eq!(aircraft.id, "3c675a");
        assert_eq!(aircraft.callsign.as_deref(), Some("DLH4AB"));
        assert_eq!(aircraft.registration.as_deref(), Some("D-AIZZ"));
        assert_eq!(aircraft.type_code.as_deref(), Some("A20N"));
        assert_eq!(aircraft.lat, 51.47);
        assert_eq!(aircraft.lon, -0.45);
        assert!(!aircraft.on_ground);
        // Geometric altitude preferred.
        assert!((aircraft.alt_m.unwrap() - 36_000.0 * 0.3048).abs() < 1e-6);
        assert!((aircraft.ground_speed_ms.unwrap() - 450.0 * 0.514_444).abs() < 1e-6);
        assert_eq!(aircraft.track_deg, Some(271.5));
        // Geometric vertical rate preferred.
        assert!((aircraft.vertical_rate_ms.unwrap() - (-640.0 * 0.005_08)).abs() < 1e-9);
        assert_eq!(aircraft.position_age_s, 0.9);
    }

    #[test]
    fn maps_category_squawk_and_flags() {
        let helicopter = only(json!({
            "ac": [{
                "hex": "abc123", "lat": 1.0, "lon": 2.0,
                "category": "A7", "squawk": 7700,
                "emergency": "general", "dbFlags": 3
            }]
        }));
        assert_eq!(helicopter.kind, AircraftKind::Helicopter);
        assert_eq!(helicopter.squawk.as_deref(), Some("7700"));
        assert_eq!(helicopter.emergency.as_deref(), Some("general"));
        assert!(helicopter.military);
        assert!(helicopter.interesting);
        assert_eq!(helicopter.unusual_reason(), Some("emergency"));

        let glider = only(json!({
            "ac": [{ "hex": "def456", "lat": 1.0, "lon": 2.0, "category": "B1" }]
        }));
        assert_eq!(glider.kind, AircraftKind::Glider);
        assert_eq!(glider.squawk, None);
        assert_eq!(glider.unusual_reason(), None);

        let balloon = only(json!({
            "ac": [{ "hex": "aaa111", "lat": 1.0, "lon": 2.0, "category": "B2" }]
        }));
        assert_eq!(balloon.kind, AircraftKind::Balloon);
    }

    #[test]
    fn flags_unusual_aircraft() {
        let squawk = only(json!({
            "ac": [{ "hex": "abc123", "lat": 1.0, "lon": 2.0, "squawk": "7700" }]
        }));
        assert_eq!(squawk.unusual_reason(), Some("emergency squawk 7700"));

        let military = only(json!({
            "ac": [{ "hex": "mil123", "lat": 1.0, "lon": 2.0, "dbFlags": 1 }]
        }));
        assert!(military.military);
        assert_eq!(military.unusual_reason(), Some("military"));

        let rare = only(json!({
            "ac": [{ "hex": "rare123", "lat": 1.0, "lon": 2.0, "t": "A400" }]
        }));
        assert_eq!(rare.unusual_reason(), Some("rare type"));

        let ordinary = only(json!({
            "ac": [{ "hex": "abc123", "lat": 1.0, "lon": 2.0, "t": "A20N", "squawk": "1000" }]
        }));
        assert_eq!(ordinary.unusual_reason(), None);
    }

    #[test]
    fn falls_back_to_barometric_altitude() {
        let aircraft = only(json!({
            "ac": [{ "hex": "abc123", "lat": 1.0, "lon": 2.0, "alt_baro": 10000 }]
        }));
        assert!((aircraft.alt_m.unwrap() - 10_000.0 * 0.3048).abs() < 1e-6);
        assert!(aircraft.callsign.is_none());
        assert_eq!(aircraft.position_age_s, 0.0);
    }

    #[test]
    fn ground_altitude_sets_on_ground_and_no_height() {
        let aircraft = only(json!({
            "ac": [{ "hex": "abc123", "lat": 1.0, "lon": 2.0, "alt_baro": "ground", "gs": 12.5 }]
        }));
        assert!(aircraft.on_ground);
        assert!(aircraft.alt_m.is_none());
        assert!(aircraft.ground_speed_ms.is_some());
    }

    #[test]
    fn empty_callsign_becomes_none() {
        let aircraft = only(json!({
            "ac": [{ "hex": "abc123", "flight": "   ", "lat": 1.0, "lon": 2.0 }]
        }));
        assert!(aircraft.callsign.is_none());
        assert_eq!(aircraft.label(), "abc123");
    }

    #[test]
    fn skips_aircraft_without_a_position() {
        let parsed = parse_response(&json!({
            "ac": [
                { "hex": "abc123" },
                { "hex": "def456", "lat": 1.0, "lon": 2.0 },
                { "hex": "aaa111", "lat": 3.0 }
            ]
        }))
        .unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "def456");
    }

    #[test]
    fn skips_aircraft_without_an_id() {
        let parsed = parse_response(&json!({
            "ac": [{ "lat": 1.0, "lon": 2.0 }]
        }))
        .unwrap();
        assert!(parsed.is_empty());
    }

    #[test]
    fn accepts_the_local_receiver_key() {
        let parsed = parse_response(&json!({
            "now": 1_700_000_000,
            "aircraft": [{ "hex": "abc123", "lat": 1.0, "lon": 2.0 }]
        }))
        .unwrap();
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn rejects_a_response_without_an_array() {
        assert!(parse_response(&json!({ "total": 0 })).is_err());
    }
}
