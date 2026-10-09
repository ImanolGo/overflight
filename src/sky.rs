//! Low-precision positions of the Moon and the bright planets.
//!
//! Method after Paul Schlyter, "How to compute planetary positions"
//! (<https://stjarnhimlen.se/comp/ppcomp.html>), which is good to a fraction of
//! a degree — plenty for a sky map. The Sun is computed too, both for the
//! geocentric geometry and as a cross-check against [`crate::sun`].

use crate::sun::Utc;

/// A celestial body we can draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body {
    Sun,
    Moon,
    Mercury,
    Venus,
    Mars,
    Jupiter,
    Saturn,
}

impl Body {
    /// The bodies drawn in the sky. The Sun is used only to light the sky.
    pub const VISIBLE: [Body; 6] = [
        Body::Moon,
        Body::Mercury,
        Body::Venus,
        Body::Mars,
        Body::Jupiter,
        Body::Saturn,
    ];

    /// The symbol drawn for this body.
    #[must_use]
    pub const fn glyph(self) -> char {
        match self {
            Body::Sun => '☉',
            Body::Moon => '☾',
            Body::Mercury => '☿',
            Body::Venus => '♀',
            Body::Mars => '♂',
            Body::Jupiter => '♃',
            Body::Saturn => '♄',
        }
    }
}

/// Where a body is in the observer's sky.
#[derive(Debug, Clone, Copy)]
pub struct BodyPosition {
    pub body: Body,
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
}

/// Compute the visible bodies' positions for an observer.
#[must_use]
pub fn positions(lat_deg: f64, lon_deg: f64, time: Utc) -> Vec<BodyPosition> {
    let jd = time.julian_day();
    Body::VISIBLE
        .iter()
        .map(|&body| {
            let (ra, dec) = equatorial(body, time);
            let (azimuth_deg, elevation_deg) = horizontal(ra, dec, lat_deg, lon_deg, jd);
            BodyPosition {
                body,
                azimuth_deg,
                elevation_deg,
            }
        })
        .collect()
}

/// Geocentric right ascension and declination, in degrees.
#[must_use]
pub fn equatorial(body: Body, time: Utc) -> (f64, f64) {
    let d = time.julian_day() - 2_451_543.5;
    let (longitude, latitude) = match body {
        Body::Sun => (sun_state(d).longitude, 0.0),
        Body::Moon => moon_ecliptic(d),
        planet => planet_ecliptic(d, planet),
    };
    let obliquity = 23.4393 - 3.563e-7 * d;
    ecliptic_to_equatorial(longitude, latitude, obliquity)
}

/// Reduce an angle to `0..360` degrees.
fn rev(degrees: f64) -> f64 {
    degrees.rem_euclid(360.0)
}

/// Solve Kepler's equation for the eccentric anomaly, in radians.
fn eccentric_anomaly(mean_anomaly_deg: f64, eccentricity: f64) -> f64 {
    let mean = mean_anomaly_deg.to_radians();
    let mut anomaly = mean + eccentricity * mean.sin() * (1.0 + eccentricity * mean.cos());
    for _ in 0..8 {
        let delta =
            (anomaly - eccentricity * anomaly.sin() - mean) / (1.0 - eccentricity * anomaly.cos());
        anomaly -= delta;
        if delta.abs() < 1e-10 {
            break;
        }
    }
    anomaly
}

struct SunState {
    x: f64,
    y: f64,
    longitude: f64,
}

/// The Sun's geocentric position in the ecliptic plane (astronomical units).
fn sun_state(d: f64) -> SunState {
    let w = 282.9404 + 4.70935e-5 * d;
    let e = 0.016_709 - 1.151e-9 * d;
    let m = rev(356.0470 + 0.985_600_258_5 * d);
    let anomaly = eccentric_anomaly(m, e);
    let xv = anomaly.cos() - e;
    let yv = (1.0 - e * e).sqrt() * anomaly.sin();
    let true_anomaly = yv.atan2(xv).to_degrees();
    let radius = (xv * xv + yv * yv).sqrt();
    let longitude = rev(true_anomaly + w);
    SunState {
        x: radius * longitude.to_radians().cos(),
        y: radius * longitude.to_radians().sin(),
        longitude,
    }
}

/// Moon geocentric ecliptic longitude and latitude, in degrees.
fn moon_ecliptic(d: f64) -> (f64, f64) {
    let n = 125.1228 - 0.052_953_808_3 * d;
    let inclination: f64 = 5.1454;
    let w = 318.0634 + 0.164_357_322_3 * d;
    let a = 60.2666; // Earth radii
    let e = 0.054_900;
    let m = rev(115.3654 + 13.064_992_950_9 * d);

    let anomaly = eccentric_anomaly(m, e);
    let xv = a * (anomaly.cos() - e);
    let yv = a * (1.0 - e * e).sqrt() * anomaly.sin();
    let true_anomaly = yv.atan2(xv).to_degrees();
    let radius = (xv * xv + yv * yv).sqrt();

    let (node, inc, vw) = (
        n.to_radians(),
        inclination.to_radians(),
        (true_anomaly + w).to_radians(),
    );
    let xh = radius * (node.cos() * vw.cos() - node.sin() * vw.sin() * inc.cos());
    let yh = radius * (node.sin() * vw.cos() + node.cos() * vw.sin() * inc.cos());
    let zh = radius * (vw.sin() * inc.sin());

    let mut longitude = rev(yh.atan2(xh).to_degrees());
    let mut latitude = zh.atan2((xh * xh + yh * yh).sqrt()).to_degrees();

    // The main periodic perturbations (Schlyter), in degrees.
    let ws = 282.9404 + 4.70935e-5 * d;
    let ms = rev(356.0470 + 0.985_600_258_5 * d);
    let mm = rev(115.3654 + 13.064_992_950_9 * d);
    let ls = rev(ms + ws); // Sun's mean longitude
    let lm = rev(mm + w + n); // Moon's mean longitude
    let dm = rev(lm - ls); // mean elongation
    let f = rev(lm - n); // argument of latitude
    let (mm, dm, ms, f) = (
        mm.to_radians(),
        dm.to_radians(),
        ms.to_radians(),
        f.to_radians(),
    );

    longitude += -1.274 * (mm - 2.0 * dm).sin() + 0.658 * (2.0 * dm).sin()
        - 0.186 * ms.sin()
        - 0.059 * (2.0 * mm - 2.0 * dm).sin()
        - 0.057 * (mm - 2.0 * dm + ms).sin()
        + 0.053 * (mm + 2.0 * dm).sin()
        + 0.046 * (2.0 * dm - ms).sin()
        + 0.041 * (mm - ms).sin()
        - 0.035 * dm.sin()
        - 0.031 * (mm + ms).sin()
        - 0.015 * (2.0 * f - 2.0 * dm).sin()
        + 0.011 * (mm - 4.0 * dm).sin();
    latitude += -0.173 * (f - 2.0 * dm).sin()
        - 0.055 * (mm - f - 2.0 * dm).sin()
        - 0.046 * (mm + f - 2.0 * dm).sin()
        + 0.033 * (f + 2.0 * dm).sin()
        + 0.017 * (2.0 * mm + f).sin();

    (rev(longitude), latitude)
}

/// A planet's geocentric ecliptic longitude and latitude, in degrees.
fn planet_ecliptic(d: f64, body: Body) -> (f64, f64) {
    let [node, inclination, w, a, e, m] = match body {
        Body::Mercury => [
            48.3313 + 3.245_87e-5 * d,
            7.0047 + 5.00e-8 * d,
            29.1241 + 1.014_44e-5 * d,
            0.387_098,
            0.205_635 + 5.59e-10 * d,
            168.6562 + 4.092_334_436_8 * d,
        ],
        Body::Venus => [
            76.6799 + 2.465_90e-5 * d,
            3.3946 + 2.75e-8 * d,
            54.8910 + 1.383_74e-5 * d,
            0.723_330,
            0.006_773 - 1.302e-9 * d,
            48.0052 + 1.602_130_224_4 * d,
        ],
        Body::Mars => [
            49.5574 + 2.110_81e-5 * d,
            1.8497 - 1.78e-8 * d,
            286.5016 + 2.929_61e-5 * d,
            1.523_688,
            0.093_405 + 2.516e-9 * d,
            18.6021 + 0.524_020_776_6 * d,
        ],
        Body::Jupiter => [
            100.4542 + 2.768_54e-5 * d,
            1.3030 - 1.557e-7 * d,
            273.8777 + 1.645_05e-5 * d,
            5.202_56,
            0.048_498 + 4.469e-9 * d,
            19.8950 + 0.083_085_300_1 * d,
        ],
        Body::Saturn => [
            113.6634 + 2.389_80e-5 * d,
            2.4886 - 1.081e-7 * d,
            339.3939 + 2.976_61e-5 * d,
            9.554_75,
            0.055_546 - 9.499e-9 * d,
            316.9670 + 0.033_444_228_2 * d,
        ],
        _ => return (0.0, 0.0),
    };

    let anomaly = eccentric_anomaly(rev(m), e);
    let xv = a * (anomaly.cos() - e);
    let yv = a * (1.0 - e * e).sqrt() * anomaly.sin();
    let true_anomaly = yv.atan2(xv).to_degrees();
    let radius = (xv * xv + yv * yv).sqrt();

    let (node, inc, vw) = (
        node.to_radians(),
        inclination.to_radians(),
        (true_anomaly + w).to_radians(),
    );
    let xh = radius * (node.cos() * vw.cos() - node.sin() * vw.sin() * inc.cos());
    let yh = radius * (node.sin() * vw.cos() + node.cos() * vw.sin() * inc.cos());
    let zh = radius * (vw.sin() * inc.sin());

    // Geocentric = heliocentric planet - heliocentric Earth, and the Sun's
    // geocentric vector is minus the Earth's.
    let sun = sun_state(d);
    let xg = xh + sun.x;
    let yg = yh + sun.y;
    let zg = zh;

    let longitude = rev(yg.atan2(xg).to_degrees());
    let latitude = zg.atan2((xg * xg + yg * yg).sqrt()).to_degrees();
    (longitude, latitude)
}

/// Ecliptic longitude/latitude to equatorial right ascension/declination.
fn ecliptic_to_equatorial(longitude_deg: f64, latitude_deg: f64, obliquity_deg: f64) -> (f64, f64) {
    let lon = longitude_deg.to_radians();
    let lat = latitude_deg.to_radians();
    let eps = obliquity_deg.to_radians();
    let ra = (lon.sin() * eps.cos() - lat.tan() * eps.sin())
        .atan2(lon.cos())
        .to_degrees()
        .rem_euclid(360.0);
    let dec = (lat.sin() * eps.cos() + lat.cos() * eps.sin() * lon.sin())
        .asin()
        .to_degrees();
    (ra, dec)
}

/// Equatorial to horizontal coordinates for an observer, degrees.
fn horizontal(
    ra_deg: f64,
    dec_deg: f64,
    lat_deg: f64,
    lon_deg: f64,
    julian_day: f64,
) -> (f64, f64) {
    let gmst = 280.460_618_37 + 360.985_647_366_29 * (julian_day - 2_451_545.0);
    let lst = rev(gmst + lon_deg);
    let mut hour_angle = rev(lst - ra_deg);
    if hour_angle > 180.0 {
        hour_angle -= 360.0;
    }

    let (sin_lat, cos_lat) = lat_deg.to_radians().sin_cos();
    let dec = dec_deg.to_radians();
    let ha = hour_angle.to_radians();
    let elevation = (sin_lat * dec.sin() + cos_lat * dec.cos() * ha.cos())
        .asin()
        .to_degrees();
    let azimuth = rev(ha
        .sin()
        .atan2(ha.cos() * sin_lat - dec.tan() * cos_lat)
        .to_degrees()
        + 180.0);
    (azimuth, elevation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn separation_deg(body_a: Body, body_b: Body, time: Utc) -> f64 {
        let (ra1, dec1) = equatorial(body_a, time);
        let (ra2, dec2) = equatorial(body_b, time);
        let (ra1, dec1, ra2, dec2) = (
            ra1.to_radians(),
            dec1.to_radians(),
            ra2.to_radians(),
            dec2.to_radians(),
        );
        (dec1.sin() * dec2.sin() + dec1.cos() * dec2.cos() * (ra1 - ra2).cos())
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees()
    }

    #[test]
    fn the_sun_agrees_with_the_noaa_elevation() {
        for time in [
            Utc::new(2024, 6, 20, 11, 8, 0),
            Utc::new(2024, 12, 21, 12, 0, 0),
            Utc::new(2025, 3, 20, 9, 30, 0),
        ] {
            let (ra, dec) = equatorial(Body::Sun, time);
            let (_, elevation) = horizontal(ra, dec, 52.52, 13.40, time.julian_day());
            let noaa = crate::sun::solar_elevation_deg(52.52, 13.40, time);
            assert!(
                (elevation - noaa).abs() < 0.6,
                "{time:?}: sky {elevation} vs noaa {noaa}"
            );
        }
    }

    #[test]
    fn the_moon_hides_the_sun_during_the_2024_eclipse() {
        // Greatest eclipse was 2024-04-08 around 18:17 UTC.
        let time = Utc::new(2024, 4, 8, 18, 18, 0);
        let separation = separation_deg(Body::Moon, Body::Sun, time);
        assert!(separation < 1.5, "eclipse separation {separation} deg");
    }

    #[test]
    fn visible_bodies_stay_in_range() {
        let time = Utc::new(2024, 6, 20, 22, 0, 0);
        let positions = positions(52.52, 13.40, time);
        assert_eq!(positions.len(), Body::VISIBLE.len());
        for position in &positions {
            assert!((0.0..360.0).contains(&position.azimuth_deg));
            assert!((-90.0..=90.0).contains(&position.elevation_deg));
        }
    }
}
