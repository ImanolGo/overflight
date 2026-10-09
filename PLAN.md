# overflight implementation plan

This document is written for a coding agent implementing overflight from scratch. Work through the milestones in order. Each milestone ends with acceptance criteria; don't start the next one until they all pass, and commit at the end of each milestone.

Read the README first for what the finished program should feel like.

## Ground rules

- Language: Rust, stable toolchain, edition 2024.
- Before every commit: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` must pass.
- **Check APIs before using them.** Crate versions and the flight-data APIs below were checked in October 2026, but both change. Read docs.rs for each crate and the provider's own docs before writing a client. If this plan disagrees with the docs, the docs win; note the difference in your commit message.
- No `unwrap()` / `expect()` outside tests and `main` setup. Use `anyhow::Result` at the edges.
- Tests must never hit the network. Everything network-facing goes behind a trait with a fixture-backed implementation.
- Be a good API citizen: send a `User-Agent` that names overflight and links the repo, never poll faster than the provider allows, and back off on errors.
- Don't add dependencies beyond the list below without a good reason written in the commit message.

## Dependencies

```toml
[dependencies]
ratatui = "0.30"
crossterm = "0.29"
reqwest = { version = "0.13", default-features = false, features = ["blocking", "json", "rustls-tls"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.9"
clap = { version = "4", features = ["derive"] }
anyhow = "1"
directories = "6"     # config file location
rand = "0.9"          # star field

[dev-dependencies]
insta = "1"
approx = "0.5"
```

Use blocking reqwest on a background thread rather than pulling in tokio. The app has one network task and doesn't need an async runtime. Double-check the reqwest feature names on docs.rs; they've been renamed in recent versions.

## Data sources

All providers implement one trait and return the same normalised type.

```rust
pub struct Aircraft {
    pub id: String,              // ICAO 24-bit hex, lowercase
    pub callsign: Option<String>, // trimmed; None if empty
    pub registration: Option<String>,
    pub type_code: Option<String>, // e.g. "A20N"
    pub lat: f64,
    pub lon: f64,
    pub alt_m: Option<f64>,      // geometric if available, else barometric
    pub on_ground: bool,
    pub ground_speed_ms: Option<f64>,
    pub track_deg: Option<f64>,  // true track, 0 = north
    pub vertical_rate_ms: Option<f64>,
    pub position_age_s: f64,     // how old the position was when the response was produced
}

pub struct Query { pub lat: f64, pub lon: f64, pub radius_km: f64 }

pub trait Provider: Send {
    fn name(&self) -> &'static str;
    fn min_interval(&self) -> std::time::Duration;
    fn fetch(&mut self, q: &Query) -> anyhow::Result<Vec<Aircraft>>;
}
```

**airplanes.live (default).** `GET https://api.airplanes.live/v2/point/{lat}/{lon}/{radius_nm}`. Radius is in nautical miles, max 250. Rate limit is 1 request per second; overflight defaults to one request every 5 seconds. No key needed at the time of writing. Non-commercial use only. The response is readsb-style JSON with an `ac` array. Fields to map: `hex`, `flight`, `r`, `t`, `lat`, `lon`, `alt_geom`, `alt_baro` (feet, and can be the string `"ground"`), `gs` (knots), `track`, `geom_rate`/`baro_rate` (ft/min), `seen_pos` (seconds). Read their field documentation before writing the parser; treat every field as optional and skip aircraft without a position.

**Local receiver.** `--source local --url http://host/data/aircraft.json`. Same readsb `aircraft.json` format (array under `aircraft` instead of `ac`), so share the parser. No rate limit; poll every second. Filter by distance yourself since the file has everything the receiver sees.

**OpenSky.** `--source opensky`. `GET https://opensky-network.org/api/states/all?lamin=..&lomin=..&lamax=..&lomax=..`. Authentication is OAuth2 client credentials only (basic auth is no longer accepted): POST to `https://auth.opensky-network.org/auth/realms/opensky-network/protocol/openid-connect/token` with `client_id` and `client_secret`, send the token as `Authorization: Bearer`, refresh it when it's close to its 30-minute expiry or on a 401. State vectors are arrays, not objects; the field order is documented on their REST page (icao24 at 0, callsign 1, longitude 5, latitude 6, baro_altitude 7, on_ground 8, velocity 9, true_track 10, vertical_rate 11, geo_altitude 13). Altitudes are metres and speeds m/s here, unlike the readsb sources. Credits: a bounding box up to 25 square degrees costs 1 credit, a standard account gets 4,000 a day, so poll no more than every 30 seconds by default and rely on dead reckoning. On 429, honour `X-Rate-Limit-Retry-After-Seconds`. Read credentials from the config file or `OPENSKY_CLIENT_ID` / `OPENSKY_CLIENT_SECRET`; never log them.

**Fixture.** `--demo` and all tests use a `FixtureProvider` that replays a recorded sequence of responses from `fixtures/` (commit one or two minutes of real airplanes.live traffic captured around a busy airport, plus a hand-written small case). Add a hidden `--record <dir>` flag that saves raw responses for making new fixtures.

## Geometry

Put all of this in `geo.rs` as pure functions with tests.

1. Convert observer and aircraft (lat, lon, altitude) to ECEF using WGS84.
2. Rotate the difference vector into the observer's local East-North-Up frame.
3. `azimuth = atan2(east, north)` normalised to 0–360°, `elevation = atan2(up, hypot(east, north))`, `slant_range = |enu|`.

This automatically accounts for Earth's curvature, which matters: an aircraft at 10 km altitude 100 km away sits at about 5.3° elevation, not 5.7°.

Tests (tolerance about 0.1°):
- Aircraft directly above the observer: elevation 90°.
- Aircraft due north at the same altitude, 50 km away: azimuth 0°, elevation slightly negative.
- 100 km due north at 10,000 m: azimuth ≈ 0°, elevation ≈ 5.3°.
- Due east and due west give 90° and 270°.
- Observer near the antimeridian (lon 179.9) with an aircraft at lon −179.9 works.

**Projection onto the screen.** Polar projection centred on the zenith: `r = (90 - elevation) / 90 * R`, angle from azimuth with north at the top. In sky mode (default) east is on the **left**, like a star chart viewed overhead; in map mode east is on the right. Terminal cells are roughly twice as tall as wide, so scale x by the cell aspect ratio (default 2.0, configurable) so the horizon is a circle, not an oval.

**Dead reckoning.** Between polls, move each aircraft along its track at its ground speed and vertical rate, starting from the position adjusted for `position_age_s`. When a fresh position arrives, don't snap: ease from the predicted position to the new one over about one second. Drop an aircraft if it hasn't been seen for 60 seconds, fading it out over the last few.

## Rendering

- The sky is a ratatui `Canvas` with Braille markers for the horizon circle, the 30° and 60° rings, and trails. Compass letters N/E/S/W sit just outside the circle.
- Aircraft are a single glyph pointing along their screen-space heading: 8 arrows `↑ ↗ → ↘ ↓ ↙ ← ↖`. The screen heading must account for the east/west flip in sky mode; write a test for it. Callsign label to the right (fall back to registration, then hex).
- Colour by altitude band: low (< 3,000 m) warm, cruise (> 9,000 m) cool, in between neutral. Climbing/descending can add a small `+` / `-` after the label.
- Trails: keep the last 60 seconds of screen positions per aircraft and draw them as fading dots.
- Sky background follows the sun: compute the sun's elevation for the observer's location with the NOAA solar position formulas (implement them, roughly 60 lines; no crate needed) and pick day / twilight / night colours. At night, draw a sparse, fixed random star field inside the circle; generate it once from a seed so it doesn't flicker.
- A status line at the bottom: data source, number of aircraft, seconds since the last update, and an error message if the last fetch failed (in the dimmest colour, not a scary red banner).
- Terminals smaller than 30×15 get a "make me bigger" message.

## App structure

```
src/
  main.rs           CLI, config loading, terminal setup/teardown, main loop
  config.rs         Config from file + CLI flags (flags win)
  app.rs            App state: tracked aircraft, selection, toggles
  fetcher.rs        background thread: polls the provider, sends results over a channel
  providers/
    mod.rs          Provider trait, Aircraft, Query
    readsb.rs       shared parser for airplanes.live and local aircraft.json
    airplanes_live.rs
    local.rs
    opensky.rs
    fixture.rs
  geo.rs            ECEF/ENU, az/el, distance
  sun.rs            solar elevation
  track.rs          per-aircraft state, dead reckoning, easing, trails
  render.rs         draws App into a ratatui Frame
fixtures/
```

**Main loop.** Target 30 fps. Poll crossterm events with a timeout of the remaining frame time, drain the fetcher channel, update tracks with the real `dt`, render. Use `ratatui::init()` / `ratatui::restore()` so a panic still restores the terminal.

**Fetcher.** Owns the provider. Sleeps for `max(provider.min_interval(), configured interval)` between requests. On error, exponential backoff up to 2 minutes, and send the error to the UI so it can show it. Stops cleanly when the channel's receiver is dropped.

**Config.** Location is required: from `--lat/--lon`, then the config file. If neither is set, print a friendly message explaining both options and exit. Don't add IP-based geolocation; it sends the user's IP to a third party and is often wrong by tens of kilometres, which matters here.

## Milestones

### M0: Project skeleton
- `cargo new --bin overflight`, dependencies, `.gitignore`, license already in repo.
- GitHub Actions running fmt, clippy and tests on ubuntu-latest, macos-latest and windows-latest.
- `main.rs` opens the alternate screen, draws the horizon circle with compass letters, and exits on `q`.

**Done when:** CI is green and the circle looks round in a typical terminal.

### M1: Geometry and sun
- `geo.rs` and `sun.rs` with all the tests above. For the sun, test against a few known values (e.g. solar noon elevation in Berlin on the June solstice is about 61°; check a published calculator for exact numbers and put the source in a comment).

**Done when:** tests pass.

### M2: Providers
- `readsb.rs` parser with tests against fixture JSON, including missing fields, `"ground"` altitude, and empty `flight`.
- airplanes.live and local providers. OpenSky provider with token handling (test token expiry logic with a fake clock).
- Hidden `--dump` flag: fetch once and print a table of aircraft with azimuth, elevation and distance, then exit.

**Done when:** `overflight --dump --lat 52.52 --lon 13.40` prints sensible traffic, and the numbers agree with a flight-tracking website for one or two planes.

### M3: Live sky
- Fetcher thread, tracks with dead reckoning and easing, projection, arrows, labels, trails, status line.
- `--demo` with the fixture provider.
- Snapshot test with ratatui's `TestBackend` and `insta` using the fixture provider and a fixed time.

**Done when:** planes move smoothly between updates with no visible jumps when a new poll arrives, and the snapshot test is stable.

### M4: Sky colours and stars
- Day, twilight and night palettes driven by `sun.rs`; star field at night.
- Hidden `--time <RFC3339>` flag to fake the current time for checking each palette.

**Done when:** all three palettes look good on a dark and a light terminal theme.

### M5: Interaction and config
- All keys from the README, detail box with direction in words ("north-east, 38° up"), units toggle, `--screensaver`, `--min-elevation`, `--radius-km`, `--interval`, `--source`, `--url`.
- Config file loading with `directories`, and a commented example config in the repo.
- `--help` text written for humans.

**Done when:** every flag and key works and `--help` reads well.

### M6: Polish and release prep
- Check overflight's own CPU use stays low (under 2% of a core) and that it handles network loss gracefully: unplug the network for a minute and plug it back in.
- Record a GIF with [vhs](https://github.com/charmbracelet/vhs) using `--demo` (so it doesn't reveal anyone's location) and put it at the top of the README in place of the ASCII mock.
- Update the README to match what was actually built.
- `CHANGELOG.md`, tag `v0.1.0`.

**Done when:** a fresh `cargo install --path .` works and the README is accurate.
