//! overflight: see the aircraft flying above you as a live sky view.

use std::cmp::Ordering;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};

use overflight::app::App;
use overflight::config::{FileConfig, Units};
use overflight::fetcher::{FetchEvent, Fetcher};
use overflight::geo;
use overflight::providers::airplanes_live::AirplanesLive;
use overflight::providers::fixture::FixtureProvider;
use overflight::providers::local::Local;
use overflight::providers::opensky::{Credentials, OpenSky};
use overflight::providers::{self, Aircraft, Provider, Query, Recorder};
use overflight::{logbook, render, satellite, sun};

/// Frame time while aircraft are moving: about 30 fps.
const MOVING_FRAME: Duration = Duration::from_millis(33);
/// Frame time when the sky is empty; only the sun and status change.
const IDLE_FRAME: Duration = Duration::from_millis(250);

const LONG_ABOUT: &str = "\
See the aircraft flying above you as a live sky view.

Tell overflight where you are with --lat/--lon or a config file, and it draws \
the planes overhead: the centre of the circle is straight up, the edge is the \
horizon, and each plane drifts across in real time with its callsign trailing \
behind. The sky is blue by day, fades at dusk, and shows stars at night.";

const AFTER_HELP: &str = "\
Examples:
  overflight --lat 52.52 --lon 13.40
  overflight --demo
  overflight --source local --url http://your-pi/data/aircraft.json
  overflight --screensaver --demo

Configuration:
  Defaults can live in config.toml in your config directory
  (~/.config/overflight/config.toml on Linux). Command-line flags win.

Keys while running:
  q / Esc   quit
  Tab       select the next aircraft
  l         show or hide callsigns
  t         show or hide trails
  m         switch sky/map orientation
  u         switch metric/imperial units";

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SourceArg {
    AirplanesLive,
    Local,
    Opensky,
}

#[derive(Debug, Parser)]
#[command(
    name = "overflight",
    version,
    about = "See the aircraft flying above you as a live sky view.",
    long_about = LONG_ABOUT,
    after_help = AFTER_HELP
)]
struct Cli {
    /// Observer latitude in decimal degrees.
    #[arg(long, allow_negative_numbers = true, value_name = "DEGREES")]
    lat: Option<f64>,

    /// Observer longitude in decimal degrees.
    #[arg(long, allow_negative_numbers = true, value_name = "DEGREES")]
    lon: Option<f64>,

    /// Observer height above sea level, in metres.
    #[arg(long, allow_negative_numbers = true, value_name = "METRES")]
    alt: Option<f64>,

    /// Where the aircraft data comes from [default: airplanes-live].
    #[arg(long, value_enum, value_name = "SOURCE")]
    source: Option<SourceArg>,

    /// URL of a local receiver's aircraft.json (with --source local).
    #[arg(long, value_name = "URL")]
    url: Option<String>,

    /// Replay recorded traffic from the bundled fixture; no network needed.
    #[arg(long)]
    demo: bool,

    /// How far out to look, in kilometres [default: 80].
    #[arg(long, allow_negative_numbers = true, value_name = "KM")]
    radius_km: Option<f64>,

    /// Ignore aircraft below this elevation, in degrees [default: 0].
    #[arg(long, allow_negative_numbers = true, value_name = "DEGREES")]
    min_elevation: Option<f64>,

    /// Seconds between updates, no faster than the source allows.
    #[arg(long, value_name = "SECONDS")]
    interval: Option<u64>,

    /// Screensaver mode: any key exits.
    #[arg(long)]
    screensaver: bool,

    /// Append a CSV line per aircraft when it leaves the sky.
    #[arg(long, value_name = "PATH")]
    log: Option<PathBuf>,

    /// Fetch once, print a table of aircraft, then exit.
    #[arg(long, hide = true)]
    dump: bool,

    /// Directory to save raw responses into, for building fixtures.
    #[arg(long, hide = true, value_name = "DIR")]
    record: Option<PathBuf>,

    /// Fake the current time as an RFC 3339 timestamp, for checking palettes.
    #[arg(long, hide = true, value_name = "RFC3339")]
    time: Option<String>,

    /// Celestrak group to draw satellites from.
    #[arg(long, hide = true, value_name = "GROUP")]
    tle_group: Option<String>,
}

/// Fully resolved settings: config file, overridden by CLI flags, with
/// built-in defaults for anything still missing.
#[derive(Debug)]
struct Settings {
    lat: Option<f64>,
    lon: Option<f64>,
    alt_m: f64,
    radius_km: f64,
    units: Units,
    min_elevation: f64,
    interval: Option<u64>,
    source: SourceArg,
    url: Option<String>,
    opensky_client_id: Option<String>,
    opensky_client_secret: Option<String>,
    demo: bool,
    dump: bool,
    record: Option<PathBuf>,
    time: Option<String>,
    tle_group: String,
    rare_types: Vec<String>,
    screensaver: bool,
    log: Option<PathBuf>,
}

impl Settings {
    fn resolve(cli: &Cli, file: Option<&FileConfig>) -> Result<Self> {
        let source = match cli.source {
            Some(source) => source,
            None => match file.and_then(|config| config.source.as_deref()) {
                Some(name) => <SourceArg as ValueEnum>::from_str(name, true)
                    .map_err(|_| anyhow::anyhow!("unknown source {name:?} in config file"))?,
                None => SourceArg::AirplanesLive,
            },
        };

        let file_lat = file.and_then(|config| config.lat);
        let file_lon = file.and_then(|config| config.lon);
        let lat = cli.lat.or(file_lat);
        let lon = cli.lon.or(file_lon);
        if let Some(lat) = lat
            && !(-90.0..=90.0).contains(&lat)
        {
            bail!(
                "latitude must be between -90 and 90 (got {lat} from {})",
                origin(cli.lat.is_some(), file_lat.is_some())
            );
        }
        if let Some(lon) = lon
            && !(-180.0..=180.0).contains(&lon)
        {
            bail!(
                "longitude must be between -180 and 180 (got {lon} from {})",
                origin(cli.lon.is_some(), file_lon.is_some())
            );
        }

        let file_radius = file.and_then(|config| config.radius_km);
        let radius_km = cli.radius_km.or(file_radius).unwrap_or(80.0);
        // 463 km is the 250 nautical mile maximum airplanes.live allows.
        if !(radius_km > 0.0 && radius_km <= 463.0) {
            bail!(
                "radius must be above 0 and at most 463 km (got {radius_km} from {})",
                origin(cli.radius_km.is_some(), file_radius.is_some())
            );
        }

        let file_min_elevation = file.and_then(|config| config.min_elevation);
        let min_elevation = cli.min_elevation.or(file_min_elevation).unwrap_or(0.0);
        if !(-5.0..=89.0).contains(&min_elevation) {
            bail!(
                "minimum elevation must be between -5 and 89 degrees (got {min_elevation} from {})",
                origin(cli.min_elevation.is_some(), file_min_elevation.is_some())
            );
        }

        Ok(Self {
            lat,
            lon,
            alt_m: cli
                .alt
                .or(file.and_then(|config| config.alt_m))
                .unwrap_or(0.0),
            radius_km,
            units: file.and_then(|config| config.units).unwrap_or_default(),
            min_elevation,
            interval: cli.interval.or(file.and_then(|config| config.interval)),
            source,
            url: cli
                .url
                .clone()
                .or_else(|| file.and_then(|config| config.url.clone())),
            opensky_client_id: file.and_then(|config| config.opensky_client_id.clone()),
            opensky_client_secret: file.and_then(|config| config.opensky_client_secret.clone()),
            demo: cli.demo,
            dump: cli.dump,
            record: cli.record.clone(),
            time: cli.time.clone(),
            tle_group: cli
                .tle_group
                .clone()
                .or_else(|| file.and_then(|config| config.tle_group.clone()))
                .unwrap_or_else(|| satellite::DEFAULT_GROUP.to_string()),
            rare_types: file
                .and_then(|config| config.rare_types.clone())
                .unwrap_or_default(),
            screensaver: cli.screensaver,
            log: cli.log.clone(),
        })
    }
}

/// Where a setting came from, for error messages.
fn origin(from_flag: bool, from_file: bool) -> &'static str {
    if from_flag {
        "the command line"
    } else if from_file {
        "the config file"
    } else {
        "the default"
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = FileConfig::load_default()?;
    let settings = Settings::resolve(&cli, config.as_ref().map(|(_, config)| config))?;

    if settings.dump {
        return dump(&settings);
    }

    // A detached fetcher thread may be blocked in an HTTP request we are not
    // waiting for. Returning from `main` normally lets the runtime tear down,
    // which can stall on that thread, so exit explicitly once the terminal has
    // been restored.
    let result = run_live(&settings);
    if let Err(error) = &result {
        eprintln!("Error: {error:#}");
    }
    std::process::exit(if result.is_ok() { 0 } else { 1 });
}

/// Build the provider and query from the resolved settings.
fn build_provider(settings: &Settings) -> Result<(Box<dyn Provider>, Query)> {
    if settings.demo {
        // The fixture was recorded around one fixed place; always use that
        // observer. Otherwise a config file's lat/lon would put every recorded
        // aircraft hundreds of kilometres away and below the horizon.
        let fixture = FixtureProvider::embedded()?;
        let query = fixture.query();
        return Ok((Box::new(fixture), query));
    }

    let (Some(lat), Some(lon)) = (settings.lat, settings.lon) else {
        bail!(
            "set your location with --lat and --lon, or in the config file \
             (~/.config/overflight/config.toml), or use --demo"
        );
    };
    let query = Query {
        lat,
        lon,
        radius_km: settings.radius_km,
        alt_m: settings.alt_m,
    };
    let client = providers::http_client()?;
    let recorder = settings.record.clone().map(Recorder::new);

    let provider: Box<dyn Provider> = match settings.source {
        SourceArg::AirplanesLive => match recorder {
            Some(recorder) => Box::new(AirplanesLive::new(client).with_recorder(recorder)),
            None => Box::new(AirplanesLive::new(client)),
        },
        SourceArg::Local => {
            let url = settings
                .url
                .clone()
                .context("--url is required with --source local")?;
            match recorder {
                Some(recorder) => Box::new(Local::new(client, url).with_recorder(recorder)),
                None => Box::new(Local::new(client, url)),
            }
        }
        SourceArg::Opensky => {
            let credentials = match (
                settings.opensky_client_id.as_ref(),
                settings.opensky_client_secret.as_ref(),
            ) {
                (Some(client_id), Some(client_secret)) => {
                    Credentials::new(client_id.clone(), client_secret.clone())
                }
                _ => Credentials::from_env()?,
            };
            match recorder {
                Some(recorder) => {
                    Box::new(OpenSky::new(client, credentials).with_recorder(recorder))
                }
                None => Box::new(OpenSky::new(client, credentials)),
            }
        }
    };

    Ok((provider, query))
}

/// Fetch once and print a table of aircraft with az/el and distance.
fn dump(settings: &Settings) -> Result<()> {
    let (mut provider, query) = build_provider(settings)?;
    let aircraft = provider.fetch(&query)?;
    print_table(provider.name(), &query, &aircraft);
    Ok(())
}

fn print_table(source: &str, query: &Query, aircraft: &[Aircraft]) {
    let observer = query.observer();
    let mut rows: Vec<(&Aircraft, geo::AzEl)> = aircraft
        .iter()
        .map(|aircraft| {
            let target =
                geo::GeoPoint::new(aircraft.lat, aircraft.lon, aircraft.alt_m.unwrap_or(0.0));
            (aircraft, geo::az_el(observer, target))
        })
        .collect();
    rows.sort_by(|a, b| {
        a.1.slant_range_m
            .partial_cmp(&b.1.slant_range_m)
            .unwrap_or(Ordering::Equal)
    });

    println!(
        "{} aircraft from {source} within {:.0} km of {:.4}, {:.4}",
        aircraft.len(),
        query.radius_km,
        query.lat,
        query.lon
    );
    println!(
        "{:<9} {:<7} {:<8} {:<5} {:>6} {:>6} {:>9} {:>8} {:>7}",
        "CALLSIGN", "HEX", "REG", "TYPE", "AZ", "ELEV", "DIST_KM", "ALT_M", "GS_MS"
    );
    for (aircraft, azel) in rows {
        let altitude = aircraft
            .alt_m
            .map_or_else(|| "-".to_string(), |alt| format!("{alt:.0}"));
        println!(
            "{:<9} {:<7} {:<8} {:<5} {:>6.1} {:>6.1} {:>9.1} {:>8} {:>7.1}",
            aircraft.label(),
            aircraft.id,
            aircraft.registration.as_deref().unwrap_or("-"),
            aircraft.type_code.as_deref().unwrap_or("-"),
            azel.azimuth_deg,
            azel.elevation_deg,
            azel.slant_range_m / 1000.0,
            altitude,
            aircraft.ground_speed_ms.unwrap_or(0.0),
        );
    }
}

/// Run the live sky view.
fn run_live(settings: &Settings) -> Result<()> {
    let (provider, query) = build_provider(settings)?;
    let source = provider.name().to_string();
    let interval = settings
        .interval
        .map(Duration::from_secs)
        .unwrap_or_else(|| provider.min_interval());
    let mut fetcher = Fetcher::spawn(provider, query, interval);

    let mut app = App::new(query, source);
    app.units = settings.units;
    app.min_elevation_deg = settings.min_elevation;
    app.rare_types = settings.rare_types.clone();
    if let Some(path) = &settings.log {
        app.set_logger(logbook::Logger::new(path));
    }
    if settings.demo {
        // Embedded TLEs are only for the offline demo; live mode uses the
        // cache, refreshed from Celestrak.
        app.set_satellites(
            satellite::embedded().unwrap_or_default(),
            now_unix_seconds(),
        );
    }

    // Refresh the TLEs from Celestrak in the background, unless offline.
    let tle_rx = if settings.demo {
        None
    } else {
        Some(spawn_tle_fetch(
            providers::http_client()?,
            settings.tle_group.clone(),
            satellite::TleCache::default_location(),
        ))
    };

    let fixed_time = settings
        .time
        .as_deref()
        .map(sun::parse_rfc3339_seconds)
        .transpose()?
        .map(|seconds| seconds as f64);

    let mut terminal = ratatui::init();
    let result = event_loop(
        &mut terminal,
        &mut app,
        &mut fetcher,
        tle_rx.as_ref(),
        fixed_time,
        settings.screensaver,
    );
    ratatui::restore();
    result
}

/// Use and, when due, refresh the TLE cache in the background.
fn spawn_tle_fetch(
    client: reqwest::blocking::Client,
    group: String,
    cache: Option<satellite::TleCache>,
) -> std::sync::mpsc::Receiver<(Vec<satellite::Satellite>, f64)> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let now = now_unix_seconds();

        // Show whatever is cached straight away; the app hides stale data.
        if let Some(cache) = &cache
            && let Some(raw) = cache.load_tle()
            && let Ok(satellites) = satellite::parse(&raw)
        {
            let fetched_at = cache.load_meta().map_or(now, |meta| meta.fetched_at);
            let _ = sender.send((satellites, fetched_at));
        }

        // Then refresh, if the cache is due (or absent).
        let outcome = match &cache {
            Some(cache) => {
                satellite::refresh(cache, now, || satellite::fetch_text(&client, &group))
            }
            None => {
                match satellite::parse(&satellite::fetch_text(&client, &group).unwrap_or_default())
                {
                    Ok(satellites) => satellite::Refresh::Fetched {
                        satellites,
                        fetched_at: now,
                    },
                    Err(_) => satellite::Refresh::Unavailable,
                }
            }
        };
        let _ = sender.send(outcome.into_parts());
    });
    receiver
}

#[allow(clippy::too_many_arguments)]
fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    fetcher: &mut Fetcher,
    tle_rx: Option<&std::sync::mpsc::Receiver<(Vec<satellite::Satellite>, f64)>>,
    fixed_time: Option<f64>,
    screensaver: bool,
) -> Result<()> {
    let start = Instant::now();
    let mut deadline = Instant::now();

    loop {
        let now_s = start.elapsed().as_secs_f64();
        let utc_s = fixed_time.unwrap_or_else(now_unix_seconds);

        while let Ok(event) = fetcher.events.try_recv() {
            match event {
                FetchEvent::Aircraft(aircraft) => app.apply(&aircraft, now_s),
                FetchEvent::Error(message) => app.set_error(message),
            }
        }
        if let Some(receiver) = tle_rx {
            while let Ok((satellites, fetched_at)) = receiver.try_recv() {
                app.set_satellites(satellites, fetched_at);
            }
        }
        app.update(now_s, utc_s);
        terminal.draw(|frame| render::render(frame, app))?;

        let frame_interval = if app.tracks.is_empty() {
            IDLE_FRAME
        } else {
            MOVING_FRAME
        };
        deadline += frame_interval;
        let timeout = deadline.saturating_duration_since(Instant::now());
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && (screensaver || handle_key(app, key.code))
        {
            break;
        }
        if Instant::now() > deadline {
            deadline = Instant::now();
        }
    }
    Ok(())
}

/// Handle a key press, returning `true` if the app should quit.
fn handle_key(app: &mut App, code: KeyCode) -> bool {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => true,
        KeyCode::Tab => {
            app.select_next();
            false
        }
        KeyCode::Char('l') => {
            app.show_callsigns = !app.show_callsigns;
            false
        }
        KeyCode::Char('t') => {
            app.show_trails = !app.show_trails;
            false
        }
        KeyCode::Char('m') => {
            app.sky_orientation = !app.sky_orientation;
            false
        }
        KeyCode::Char('h') => {
            app.horizon = !app.horizon;
            false
        }
        KeyCode::Left => {
            app.view_azimuth_deg = (app.view_azimuth_deg - 45.0).rem_euclid(360.0);
            false
        }
        KeyCode::Right => {
            app.view_azimuth_deg = (app.view_azimuth_deg + 45.0).rem_euclid(360.0);
            false
        }
        KeyCode::Char('u') => {
            app.units = app.units.toggled();
            false
        }
        KeyCode::Char('c') => {
            app.constellation_lines = !app.constellation_lines;
            false
        }
        _ => false,
    }
}

/// Seconds since the Unix epoch.
fn now_unix_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> FileConfig {
        FileConfig {
            lat: Some(1.0),
            lon: Some(2.0),
            radius_km: Some(10.0),
            units: Some(Units::Imperial),
            min_elevation: Some(5.0),
            interval: Some(9),
            source: Some("local".to_string()),
            url: Some("http://example/aircraft.json".to_string()),
            ..FileConfig::default()
        }
    }

    #[test]
    fn command_line_flags_override_the_config_file() {
        let cli = Cli::parse_from(["overflight", "--lat", "40.0", "--radius-km", "20"]);
        let settings = Settings::resolve(&cli, Some(&config())).unwrap();

        assert_eq!(settings.lat, Some(40.0)); // CLI wins
        assert_eq!(settings.lon, Some(2.0)); // from the file
        assert_eq!(settings.radius_km, 20.0); // CLI wins
        assert_eq!(settings.units, Units::Imperial); // from the file
        assert_eq!(settings.interval, Some(9));
        assert_eq!(settings.min_elevation, 5.0);
        assert!(matches!(settings.source, SourceArg::Local));
        assert_eq!(
            settings.url.as_deref(),
            Some("http://example/aircraft.json")
        );
    }

    #[test]
    fn defaults_apply_without_a_config_file() {
        let cli = Cli::parse_from(["overflight"]);
        let settings = Settings::resolve(&cli, None).unwrap();
        assert_eq!(settings.radius_km, 80.0);
        assert_eq!(settings.min_elevation, 0.0);
        assert_eq!(settings.units, Units::Metric);
        assert!(matches!(settings.source, SourceArg::AirplanesLive));
    }

    #[test]
    fn rejects_out_of_range_location_and_radius() {
        for args in [
            vec!["overflight", "--lat", "200"],
            vec!["overflight", "--lat", "-91"],
            vec!["overflight", "--lon", "-500"],
            vec!["overflight", "--lon", "181"],
            vec!["overflight", "--radius-km", "0"],
            vec!["overflight", "--radius-km", "-50"],
            vec!["overflight", "--radius-km", "500"],
            vec!["overflight", "--radius-km", "NaN"],
            vec!["overflight", "--min-elevation", "90"],
            vec!["overflight", "--min-elevation", "-40"],
        ] {
            let cli = Cli::parse_from(args.iter().copied());
            let error = Settings::resolve(&cli, None)
                .expect_err("expected a validation error")
                .to_string();
            assert!(error.contains("command line"), "for {args:?}: {error}");
        }
    }

    #[test]
    fn validation_names_the_config_file_as_the_source() {
        let file = FileConfig {
            radius_km: Some(-5.0),
            ..FileConfig::default()
        };
        let cli = Cli::parse_from(["overflight"]);
        let error = Settings::resolve(&cli, Some(&file))
            .expect_err("expected a validation error")
            .to_string();
        assert!(error.contains("radius"), "{error}");
        assert!(error.contains("config file"), "{error}");
    }

    #[test]
    fn an_unknown_config_source_is_an_error() {
        let cli = Cli::parse_from(["overflight"]);
        let file = FileConfig {
            source: Some("nope".to_string()),
            ..FileConfig::default()
        };
        assert!(Settings::resolve(&cli, Some(&file)).is_err());
    }

    #[test]
    fn demo_uses_the_fixture_observer_not_the_config() {
        let cli = Cli::parse_from(["overflight", "--demo"]);
        let settings = Settings::resolve(&cli, Some(&config())).unwrap();
        let (provider, query) = build_provider(&settings).unwrap();
        assert_eq!(provider.name(), "demo");
        // The fixture was recorded near Heathrow; the config's lat/lon is ignored.
        assert!((query.lat - 51.47).abs() < 0.01, "lat {}", query.lat);
        assert!((query.lon + 0.4543).abs() < 0.01, "lon {}", query.lon);
    }

    #[test]
    fn keys_toggle_app_state_and_quit() {
        let mut app = App::new(
            Query {
                lat: 0.0,
                lon: 0.0,
                radius_km: 1.0,
                alt_m: 0.0,
            },
            "test",
        );
        assert!(handle_key(&mut app, KeyCode::Char('q')));
        assert!(app.show_callsigns);
        assert!(!handle_key(&mut app, KeyCode::Char('l')));
        assert!(!app.show_callsigns);
        assert!(!handle_key(&mut app, KeyCode::Char('t')));
        assert!(!app.show_trails);
        assert!(!handle_key(&mut app, KeyCode::Char('m')));
        assert!(!app.sky_orientation);
        assert!(!handle_key(&mut app, KeyCode::Char('u')));
        assert_eq!(app.units, Units::Imperial);
        assert!(!handle_key(&mut app, KeyCode::Tab));
        assert_eq!(app.selected, None);
        assert!(handle_key(&mut app, KeyCode::Esc));
    }
}
