//! overflight: see the aircraft flying above you as a live sky view.

use std::cmp::Ordering;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};

use overflight::app::App;
use overflight::fetcher::{FetchEvent, Fetcher};
use overflight::geo;
use overflight::providers::airplanes_live::AirplanesLive;
use overflight::providers::fixture::FixtureProvider;
use overflight::providers::local::Local;
use overflight::providers::opensky::{Credentials, OpenSky};
use overflight::providers::{self, Aircraft, Provider, Query, Recorder};
use overflight::render;

/// Target frame time, about 30 fps.
const TARGET_FRAME: Duration = Duration::from_millis(33);

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
    about = "See the aircraft flying above you as a live sky view."
)]
struct Cli {
    /// Observer latitude in decimal degrees.
    #[arg(long, allow_negative_numbers = true)]
    lat: Option<f64>,

    /// Observer longitude in decimal degrees.
    #[arg(long, allow_negative_numbers = true)]
    lon: Option<f64>,

    /// Where the aircraft data comes from.
    #[arg(long, value_enum, default_value = "airplanes-live")]
    source: SourceArg,

    /// URL of a local receiver's aircraft.json (with --source local).
    #[arg(long)]
    url: Option<String>,

    /// Replay recorded traffic from the bundled fixture; no network needed.
    #[arg(long)]
    demo: bool,

    /// How far out to look, in kilometres.
    #[arg(long, default_value_t = 80.0)]
    radius_km: f64,

    /// Fetch once, print a table of aircraft, then exit.
    #[arg(long, hide = true)]
    dump: bool,

    /// Directory to save raw responses into, for building fixtures.
    #[arg(long, hide = true, value_name = "DIR")]
    record: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.dump {
        return dump(&cli);
    }
    run_live(&cli)
}

/// Build the provider and query from the CLI.
fn build_provider(cli: &Cli) -> Result<(Box<dyn Provider>, Query)> {
    if cli.demo {
        let fixture = FixtureProvider::embedded()?;
        let query = match (cli.lat, cli.lon) {
            (Some(lat), Some(lon)) => Query {
                lat,
                lon,
                radius_km: cli.radius_km,
            },
            (None, None) => fixture.query(),
            _ => bail!("give both --lat and --lon, or neither with --demo"),
        };
        return Ok((Box::new(fixture), query));
    }

    let (Some(lat), Some(lon)) = (cli.lat, cli.lon) else {
        bail!("set your location with --lat and --lon, or use --demo");
    };
    let query = Query {
        lat,
        lon,
        radius_km: cli.radius_km,
    };
    let client = providers::http_client()?;
    let recorder = cli.record.clone().map(Recorder::new);

    let provider: Box<dyn Provider> = match cli.source {
        SourceArg::AirplanesLive => match recorder {
            Some(recorder) => Box::new(AirplanesLive::new(client).with_recorder(recorder)),
            None => Box::new(AirplanesLive::new(client)),
        },
        SourceArg::Local => {
            let url = cli
                .url
                .clone()
                .context("--url is required with --source local")?;
            match recorder {
                Some(recorder) => Box::new(Local::new(client, url).with_recorder(recorder)),
                None => Box::new(Local::new(client, url)),
            }
        }
        SourceArg::Opensky => {
            let credentials = Credentials::from_env()?;
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
fn dump(cli: &Cli) -> Result<()> {
    let (mut provider, query) = build_provider(cli)?;
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
fn run_live(cli: &Cli) -> Result<()> {
    let (provider, query) = build_provider(cli)?;
    let source = provider.name().to_string();
    let interval = provider.min_interval();
    let mut fetcher = Fetcher::spawn(provider, query, interval);
    let mut app = App::new(query, source);

    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app, &mut fetcher);
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    fetcher: &mut Fetcher,
) -> Result<()> {
    let start = Instant::now();
    let mut deadline = Instant::now();

    loop {
        let now_s = start.elapsed().as_secs_f64();

        while let Ok(event) = fetcher.events.try_recv() {
            match event {
                FetchEvent::Aircraft(aircraft) => app.apply(&aircraft, now_s),
                FetchEvent::Error(message) => app.set_error(message),
            }
        }
        app.update(now_s);
        terminal.draw(|frame| render::render(frame, app))?;

        deadline += TARGET_FRAME;
        let timeout = deadline.saturating_duration_since(Instant::now());
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && matches!(key.code, KeyCode::Char('q') | KeyCode::Esc)
        {
            break;
        }
        if Instant::now() > deadline {
            deadline = Instant::now();
        }
    }
    Ok(())
}
