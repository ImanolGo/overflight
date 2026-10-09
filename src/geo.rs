//! Geodesy: turn geodetic positions into local azimuth, elevation and range.
//!
//! Positions are converted to Earth-Centred, Earth-Fixed (ECEF) coordinates on
//! the WGS84 ellipsoid, then the vector between observer and target is rotated
//! into the observer's local East-North-Up frame. Working in ECEF means Earth
//! curvature is handled for free: the local horizontal plane is tangent to the
//! ellipsoid rather than to a flat Earth.

/// WGS84 semi-major axis, in metres.
const WGS84_A: f64 = 6_378_137.0;
/// WGS84 flattening.
const WGS84_F: f64 = 1.0 / 298.257_223_563;
/// WGS84 first eccentricity squared.
const WGS84_E2: f64 = WGS84_F * (2.0 - WGS84_F);

/// A position on (or above) the WGS84 ellipsoid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoPoint {
    /// Geodetic latitude, degrees north.
    pub lat_deg: f64,
    /// Longitude, degrees east.
    pub lon_deg: f64,
    /// Height above the ellipsoid, metres.
    pub alt_m: f64,
}

impl GeoPoint {
    /// Build a point.
    #[must_use]
    pub const fn new(lat_deg: f64, lon_deg: f64, alt_m: f64) -> Self {
        Self {
            lat_deg,
            lon_deg,
            alt_m,
        }
    }
}

/// Where to look for a target, as seen by an observer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AzEl {
    /// Compass azimuth, degrees clockwise from true north (`0..360`).
    pub azimuth_deg: f64,
    /// Elevation above the horizon, degrees (`90` is straight up).
    pub elevation_deg: f64,
    /// Straight-line distance to the target, metres.
    pub slant_range_m: f64,
}

/// Earth-Centred, Earth-Fixed position, in metres.
fn to_ecef(p: GeoPoint) -> [f64; 3] {
    let lat = p.lat_deg.to_radians();
    let lon = p.lon_deg.to_radians();
    let (sin_lat, cos_lat) = lat.sin_cos();
    let (sin_lon, cos_lon) = lon.sin_cos();
    let n = WGS84_A / (1.0 - WGS84_E2 * sin_lat * sin_lat).sqrt();
    [
        (n + p.alt_m) * cos_lat * cos_lon,
        (n + p.alt_m) * cos_lat * sin_lon,
        (n * (1.0 - WGS84_E2) + p.alt_m) * sin_lat,
    ]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Offset from `observer` to `target` in the observer's East-North-Up frame,
/// in metres.
#[must_use]
pub fn enu(observer: GeoPoint, target: GeoPoint) -> [f64; 3] {
    enu_from_ecef(observer, to_ecef(target))
}

/// Offset from `observer` to an ECEF point (metres), in the observer's
/// East-North-Up frame.
#[must_use]
pub fn enu_from_ecef(observer: GeoPoint, target: [f64; 3]) -> [f64; 3] {
    let o = to_ecef(observer);
    let d = [target[0] - o[0], target[1] - o[1], target[2] - o[2]];

    let lat = observer.lat_deg.to_radians();
    let lon = observer.lon_deg.to_radians();
    let (sin_lat, cos_lat) = lat.sin_cos();
    let (sin_lon, cos_lon) = lon.sin_cos();

    let east = [-sin_lon, cos_lon, 0.0];
    let north = [-sin_lat * cos_lon, -sin_lat * sin_lon, cos_lat];
    let up = [cos_lat * cos_lon, cos_lat * sin_lon, sin_lat];

    [dot(d, east), dot(d, north), dot(d, up)]
}

/// Azimuth, elevation and slant range from `observer` to `target`.
#[must_use]
pub fn az_el(observer: GeoPoint, target: GeoPoint) -> AzEl {
    let [east, north, up] = enu(observer, target);
    let ground_range = east.hypot(north);
    AzEl {
        azimuth_deg: east.atan2(north).to_degrees().rem_euclid(360.0),
        elevation_deg: up.atan2(ground_range).to_degrees(),
        slant_range_m: (east * east + north * north + up * up).sqrt(),
    }
}

/// Project a direction onto the sky disc, in units of the horizon radius.
///
/// Centre is the zenith, edge is the horizon: `r = (90 - elevation) / 90`.
/// `+y` is north and `+x` is east in map orientation. In sky orientation
/// (default) east is mirrored onto the left, as the sky looks from below.
#[must_use]
pub fn project(azimuth_deg: f64, elevation_deg: f64, sky_orientation: bool) -> (f64, f64) {
    let radius = (90.0 - elevation_deg) / 90.0;
    let azimuth = azimuth_deg.to_radians();
    let x = if sky_orientation {
        -radius * azimuth.sin()
    } else {
        radius * azimuth.sin()
    };
    (x, radius * azimuth.cos())
}

#[cfg(test)]
mod tests {
    use approx::assert_abs_diff_eq;

    use super::*;

    /// Mean Earth radius used to place test targets on the sphere.
    const EARTH_R: f64 = 6_371_000.0;

    /// A point `distance_m` away from `start` along the given bearing.
    fn destination(start: GeoPoint, bearing_deg: f64, distance_m: f64, alt_m: f64) -> GeoPoint {
        let angular = distance_m / EARTH_R;
        let bearing = bearing_deg.to_radians();
        let lat1 = start.lat_deg.to_radians();
        let lon1 = start.lon_deg.to_radians();
        let lat2 = (lat1.sin() * angular.cos() + lat1.cos() * angular.sin() * bearing.cos()).asin();
        let lon2 = lon1
            + (bearing.sin() * angular.sin() * lat1.cos())
                .atan2(angular.cos() - lat1.sin() * lat2.sin());
        GeoPoint::new(lat2.to_degrees(), lon2.to_degrees(), alt_m)
    }

    fn berlin() -> GeoPoint {
        GeoPoint::new(52.52, 13.40, 0.0)
    }

    /// Angular distance between two azimuths, accounting for the 0/360 wrap.
    fn azimuth_difference(a: f64, b: f64) -> f64 {
        let d = (a - b).rem_euclid(360.0);
        d.min(360.0 - d)
    }

    fn assert_azimuth(actual: f64, expected: f64) {
        let diff = azimuth_difference(actual, expected);
        assert!(diff < 0.1, "azimuth {actual} is {diff} deg from {expected}");
    }

    #[test]
    fn directly_overhead_is_ninety_degrees() {
        let observer = berlin();
        let target = GeoPoint::new(52.52, 13.40, 10_000.0);
        let azel = az_el(observer, target);
        assert_abs_diff_eq!(azel.elevation_deg, 90.0, epsilon = 1e-6);
        assert_abs_diff_eq!(azel.slant_range_m, 10_000.0, epsilon = 1.0);
    }

    #[test]
    fn due_north_at_same_altitude_is_slightly_below_horizon() {
        let observer = berlin();
        let target = destination(observer, 0.0, 50_000.0, 0.0);
        let azel = az_el(observer, target);
        assert_azimuth(azel.azimuth_deg, 0.0);
        assert!(
            azel.elevation_deg < 0.0 && azel.elevation_deg > -0.5,
            "expected just below the horizon, got {}",
            azel.elevation_deg
        );
    }

    #[test]
    fn due_north_100km_at_10km_matches_curvature() {
        let observer = berlin();
        let target = destination(observer, 0.0, 100_000.0, 10_000.0);
        let azel = az_el(observer, target);
        assert_azimuth(azel.azimuth_deg, 0.0);
        // 5.3°, not the flat-Earth 5.7°.
        assert_abs_diff_eq!(azel.elevation_deg, 5.3, epsilon = 0.1);
    }

    #[test]
    fn due_east_and_west_are_ninety_and_270() {
        let observer = berlin();
        let east = destination(observer, 90.0, 50_000.0, 5_000.0);
        let west = destination(observer, 270.0, 50_000.0, 5_000.0);
        assert_azimuth(az_el(observer, east).azimuth_deg, 90.0);
        assert_azimuth(az_el(observer, west).azimuth_deg, 270.0);
    }

    #[test]
    fn works_across_the_antimeridian() {
        let observer = GeoPoint::new(0.0, 179.9, 0.0);
        let target = GeoPoint::new(0.0, -179.9, 5_000.0);
        let azel = az_el(observer, target);
        assert_azimuth(azel.azimuth_deg, 90.0);
        assert!(azel.elevation_deg > 0.0);
    }

    #[test]
    fn projection_puts_zenith_at_the_centre() {
        let (x, y) = project(123.0, 90.0, true);
        assert_abs_diff_eq!(x, 0.0, epsilon = 1e-9);
        assert_abs_diff_eq!(y, 0.0, epsilon = 1e-9);
    }

    #[test]
    fn projection_puts_the_horizon_on_the_edge() {
        assert_abs_diff_eq!(project(0.0, 0.0, true).1, 1.0, epsilon = 1e-9);
        assert_abs_diff_eq!(project(180.0, 0.0, true).1, -1.0, epsilon = 1e-9);
    }

    #[test]
    fn projection_mirrors_east_in_sky_orientation() {
        let (sky_x, sky_y) = project(90.0, 0.0, true);
        let (map_x, map_y) = project(90.0, 0.0, false);
        assert_abs_diff_eq!(sky_x, -1.0, epsilon = 1e-9);
        assert_abs_diff_eq!(map_x, 1.0, epsilon = 1e-9);
        assert_abs_diff_eq!(sky_y, 0.0, epsilon = 1e-9);
        assert_abs_diff_eq!(map_y, 0.0, epsilon = 1e-9);
    }

    #[test]
    fn observer_altitude_puts_low_traffic_near_the_horizon() {
        // Denver, 1650 m: a plane on approach 5 km away at 1830 m is about 2°
        // up, not the 20° you get by pretending the observer is at sea level.
        let observer = GeoPoint::new(39.7392, -104.9903, 1650.0);
        let target = destination(observer, 0.0, 5000.0, 1830.0);
        let azel = az_el(observer, target);
        assert_abs_diff_eq!(azel.elevation_deg, 2.0, epsilon = 0.5);
    }
}
