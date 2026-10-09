//! OpenSky Network provider.
//!
//! OpenSky uses OAuth2 client credentials: exchange `client_id`/`client_secret`
//! for a bearer token, refresh it before its 30-minute expiry or on a `401`,
//! and respect `429` rate limiting. State vectors are arrays, not objects, and
//! altitudes/speeds are metric here, unlike the readsb sources.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;

use super::{Aircraft, Provider, Query, Recorder};

const AUTH_URL: &str =
    "https://auth.opensky-network.org/auth/realms/opensky-network/protocol/openid-connect/token";
const BASE_URL: &str = "https://opensky-network.org/api";
/// Refresh the token this many seconds before it actually expires.
const REFRESH_MARGIN_S: f64 = 60.0;

/// Read the current time. Split out so token handling can be tested with a
/// fake clock.
pub trait Clock: Send {
    /// Seconds since the Unix epoch.
    fn now_secs(&self) -> f64;
}

/// The real system clock.
#[derive(Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_secs(&self) -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs_f64())
            .unwrap_or(0.0)
    }
}

/// OpenSky API client credentials.
#[derive(Clone)]
pub struct Credentials {
    client_id: String,
    client_secret: String,
}

impl Credentials {
    /// Build credentials from a client id and secret.
    #[must_use]
    pub fn new(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret: client_secret.into(),
        }
    }

    /// Read credentials from `OPENSKY_CLIENT_ID` and `OPENSKY_CLIENT_SECRET`.
    pub fn from_env() -> Result<Self> {
        let client_id = std::env::var("OPENSKY_CLIENT_ID")
            .context("OPENSKY_CLIENT_ID is not set (or use --source opensky with config)")?;
        let client_secret = std::env::var("OPENSKY_CLIENT_SECRET")
            .context("OPENSKY_CLIENT_SECRET is not set (or use --source opensky with config)")?;
        if client_id.is_empty() || client_secret.is_empty() {
            bail!("OpenSky credentials are empty");
        }
        Ok(Self {
            client_id,
            client_secret,
        })
    }
}

impl std::fmt::Debug for Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credentials")
            .field("client_id", &self.client_id)
            .field("client_secret", &"<redacted>")
            .finish()
    }
}

/// Cached bearer token with its expiry.
#[derive(Debug, Default)]
struct TokenCache {
    token: Option<String>,
    expires_at_s: f64,
}

impl TokenCache {
    /// A token usable at `now`, or `None` if one must be fetched.
    fn valid_token(&self, now: f64) -> Option<&str> {
        match &self.token {
            Some(token) if now < self.expires_at_s - REFRESH_MARGIN_S => Some(token),
            _ => None,
        }
    }

    fn store(&mut self, token: String, expires_in_s: f64, now: f64) {
        self.token = Some(token);
        self.expires_at_s = now + expires_in_s;
    }

    fn invalidate(&mut self) {
        self.token = None;
        self.expires_at_s = 0.0;
    }
}

/// The OpenSky provider.
pub struct OpenSky {
    client: Client,
    auth_url: String,
    base_url: String,
    credentials: Credentials,
    clock: Box<dyn Clock>,
    tokens: TokenCache,
    recorder: Option<Recorder>,
}

impl OpenSky {
    /// Create the provider against the public API.
    #[must_use]
    pub fn new(client: Client, credentials: Credentials) -> Self {
        Self {
            client,
            auth_url: AUTH_URL.to_string(),
            base_url: BASE_URL.to_string(),
            credentials,
            clock: Box::<SystemClock>::default(),
            tokens: TokenCache::default(),
            recorder: None,
        }
    }

    /// Use a specific clock (for tests).
    #[must_use]
    pub fn with_clock(mut self, clock: Box<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Also save each raw response with `recorder`.
    #[must_use]
    pub fn with_recorder(mut self, recorder: Recorder) -> Self {
        self.recorder = Some(recorder);
        self
    }

    fn refresh_token(&mut self, now: f64) -> Result<()> {
        let body = format!(
            "grant_type=client_credentials&client_id={}&client_secret={}",
            form_urlencode(&self.credentials.client_id),
            form_urlencode(&self.credentials.client_secret),
        );
        let response = self
            .client
            .post(&self.auth_url)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(body)
            .send()
            .context("OpenSky token request")?
            .error_for_status()
            .context("OpenSky token request failed")?;
        let raw = response.text().context("reading OpenSky token response")?;
        let token: TokenResponse =
            serde_json::from_str(&raw).context("OpenSky token response was not JSON")?;
        self.tokens.store(token.access_token, token.expires_in, now);
        Ok(())
    }
}

impl Provider for OpenSky {
    fn name(&self) -> &'static str {
        "opensky"
    }

    fn min_interval(&self) -> Duration {
        Duration::from_secs(30)
    }

    fn fetch(&mut self, query: &Query) -> Result<Vec<Aircraft>> {
        let url = states_url(&self.base_url, query);

        for attempt in 0..2 {
            let now = self.clock.now_secs();
            if self.tokens.valid_token(now).is_none() {
                self.refresh_token(now)?;
            }
            let token = self
                .tokens
                .valid_token(self.clock.now_secs())
                .ok_or_else(|| anyhow!("no OpenSky token available"))?
                .to_string();

            let response = self
                .client
                .get(&url)
                .bearer_auth(&token)
                .send()
                .context("OpenSky request")?;
            let status = response.status();

            if status == reqwest::StatusCode::UNAUTHORIZED && attempt == 0 {
                self.tokens.invalidate();
                continue;
            }
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let retry = response
                    .headers()
                    .get("X-Rate-Limit-Retry-After-Seconds")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("unknown");
                bail!("OpenSky rate limit reached (retry after {retry}s)");
            }

            let raw = response
                .error_for_status()
                .context("OpenSky request failed")?
                .text()
                .context("reading OpenSky response")?;
            if let Some(recorder) = &mut self.recorder {
                recorder.record(&raw)?;
            }
            let value: Value =
                serde_json::from_str(&raw).context("OpenSky response was not JSON")?;
            return Ok(parse_states(&value)
                .into_iter()
                .filter(|aircraft| query.contains(aircraft))
                .collect());
        }

        bail!("OpenSky rejected the access token twice")
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default = "default_expires_in")]
    expires_in: f64,
}

fn default_expires_in() -> f64 {
    1800.0
}

/// Build the `states/all` URL for a bounding box around the query.
fn states_url(base_url: &str, query: &Query) -> String {
    let (lamin, lomin, lamax, lomax) = bounding_box(query);
    format!(
        "{base_url}/states/all?lamin={lamin:.4}&lomin={lomin:.4}&lamax={lamax:.4}&lomax={lomax:.4}"
    )
}

/// A latitude/longitude bounding box `(lamin, lomin, lamax, lomax)` around the
/// query, sized so it stays within the one-credit quota where possible.
fn bounding_box(query: &Query) -> (f64, f64, f64, f64) {
    const EARTH_R: f64 = 6_371_000.0;
    // A generous latitude delta keeps the box at least as large as the circle.
    let dlat = (query.radius_km * 1000.0 / EARTH_R).to_degrees();
    let cos_lat = query.lat.to_radians().cos().abs().max(0.01);
    let dlon = (dlat / cos_lat).min(180.0);
    (
        (query.lat - dlat).max(-90.0),
        (query.lon - dlon).max(-180.0),
        (query.lat + dlat).min(90.0),
        (query.lon + dlon).min(180.0),
    )
}

/// Parse an OpenSky `states/all` response into normalized aircraft.
#[must_use]
pub fn parse_states(value: &Value) -> Vec<Aircraft> {
    let response_time = value.get("time").and_then(Value::as_f64);
    let Some(states) = value.get("states").and_then(Value::as_array) else {
        return Vec::new();
    };
    states
        .iter()
        .filter_map(|state| parse_state(state, response_time))
        .collect()
}

fn parse_state(state: &Value, response_time: Option<f64>) -> Option<Aircraft> {
    let fields = state.as_array()?;
    let field = |index: usize| fields.get(index);
    let number = |index: usize| field(index).and_then(Value::as_f64);

    let id = field(0)?.as_str()?.to_lowercase();
    let lat = number(6)?;
    let lon = number(5)?;

    let callsign = field(1)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|callsign| !callsign.is_empty())
        .map(str::to_string);

    let position_age_s = match (response_time, number(3)) {
        (Some(now), Some(updated)) => (now - updated).max(0.0),
        _ => 0.0,
    };

    Some(Aircraft {
        id,
        callsign,
        registration: None,
        type_code: None,
        lat,
        lon,
        // Geometric altitude if available, else barometric. Both in metres.
        alt_m: number(13).or_else(|| number(7)),
        on_ground: field(8).and_then(Value::as_bool).unwrap_or(false),
        ground_speed_ms: number(9),
        track_deg: number(10),
        vertical_rate_ms: number(11),
        position_age_s,
    })
}

/// Percent-encode a value for an `application/x-www-form-urlencoded` body.
fn form_urlencode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A bounding box up to this many square degrees costs a single credit.
    const CREDIT_FREE_AREA_SQ_DEG: f64 = 25.0;

    fn query() -> Query {
        Query {
            lat: 52.52,
            lon: 13.40,
            radius_km: 80.0,
        }
    }

    fn bounding_box_area_sq_deg(query: &Query) -> f64 {
        let (lamin, lomin, lamax, lomax) = bounding_box(query);
        (lamax - lamin) * (lomax - lomin)
    }

    #[test]
    fn token_is_valid_until_close_to_expiry() {
        let mut cache = TokenCache::default();
        assert!(cache.valid_token(0.0).is_none());

        cache.store("abc".to_string(), 1800.0, 1000.0);
        assert_eq!(cache.valid_token(1001.0), Some("abc"));
        assert_eq!(cache.valid_token(2739.9), Some("abc"));
        // Within the refresh margin (expires at 2800, margin 60).
        assert_eq!(cache.valid_token(2740.0), None);
        assert_eq!(cache.valid_token(3000.0), None);

        cache.invalidate();
        assert_eq!(cache.valid_token(1001.0), None);
    }

    #[test]
    fn parses_a_state_vector() {
        let states = parse_states(&json!({
            "time": 1_700_000_010,
            "states": [[
                "3c675a", "DLH4AB  ", "Germany",
                1_700_000_000, 1_700_000_010,
                13.40, 52.52,
                10668.0, false, 231.5, 271.0, -3.0, null, 10972.8,
                "1000", false, 0
            ]]
        }));
        assert_eq!(states.len(), 1);
        let aircraft = &states[0];
        assert_eq!(aircraft.id, "3c675a");
        assert_eq!(aircraft.callsign.as_deref(), Some("DLH4AB"));
        assert_eq!(aircraft.lat, 52.52);
        assert_eq!(aircraft.lon, 13.40);
        // Geometric altitude preferred over barometric.
        assert_eq!(aircraft.alt_m, Some(10972.8));
        assert_eq!(aircraft.ground_speed_ms, Some(231.5));
        assert_eq!(aircraft.track_deg, Some(271.0));
        assert_eq!(aircraft.vertical_rate_ms, Some(-3.0));
        assert_eq!(aircraft.position_age_s, 10.0);
    }

    #[test]
    fn skips_states_without_a_position() {
        let states = parse_states(&json!({
            "time": 1,
            "states": [["abc123", "X", "Y", null, null, null, null, null, false, null, null, null, null, null, null, false, 0]]
        }));
        assert!(states.is_empty());
    }

    #[test]
    fn builds_a_bounding_box_around_the_query() {
        let (lamin, lomin, lamax, lomax) = bounding_box(&query());
        assert!(lamin < 52.52 && lamax > 52.52);
        assert!(lomin < 13.40 && lomax > 13.40);
        // 80 km is about 0.72 degrees of latitude.
        assert!(
            (lamax - lamin - 1.44).abs() < 0.05,
            "lat span {}",
            lamax - lamin
        );
    }

    #[test]
    fn a_default_radius_stays_within_one_credit() {
        assert!(bounding_box_area_sq_deg(&query()) <= CREDIT_FREE_AREA_SQ_DEG);
    }

    #[test]
    fn states_url_includes_the_bounding_box() {
        let url = states_url("https://opensky-network.org/api", &query());
        assert!(url.starts_with("https://opensky-network.org/api/states/all?"));
        assert!(url.contains("lamin="));
        assert!(url.contains("lomax="));
    }

    #[test]
    fn form_encoding_matches_urlencoded_rules() {
        assert_eq!(form_urlencode("a b&c=d/e"), "a+b%26c%3Dd%2Fe");
        assert_eq!(form_urlencode("AZaz09-_.~"), "AZaz09-_.~");
    }
}
