//! The command-line interface, shared with the man-page and completion
//! generator so those are always built from the same definition.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::render::ColorMode;

const LONG_ABOUT: &str = "\
See the aircraft flying above you as a live sky view.

Tell overflight where you are with --lat/--lon or a config file, and it draws \
the planes overhead: the centre of the circle is straight up, the edge is the \
horizon, and each plane drifts across in real time with its callsign trailing \
behind. The sky is blue by day, fades at dusk, and at night shows the real \
stars and constellations, the Moon, the bright planets and passing satellites.";

const AFTER_HELP: &str = "\
Examples:
  overflight --lat 52.52 --lon 13.40
  overflight --demo
  overflight --source local --url http://your-pi/data/aircraft.json
  overflight --log ~/flights.csv --bell
  overflight --screensaver --demo

Configuration:
  Defaults can live in config.toml in your config directory
  (~/.config/overflight/config.toml on Linux). Command-line flags win.

Keys while running:
  q / Esc        quit
  Tab            select the next aircraft or satellite and show its details
  l / t          show or hide callsigns / trails
  m              switch between sky and map orientation
  h              side-on horizon view; Left/Right turn it
  c              show or hide constellation lines
  u              switch between metric and imperial
  click          select the nearest aircraft or satellite (--no-mouse to disable)

The status line shows how many aircraft are in range, the next aircraft due \
overhead, and the next visible ISS pass at twilight. Military aircraft, \
emergency squawks and rare types are highlighted, and --bell can ring for \
them. When there is a network, the selected aircraft's route is looked up.";

/// The aircraft data sources.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SourceArg {
    AdsbLol,
    AirplanesLive,
    Local,
    Opensky,
}

/// How much colour to use, matching the `--colors` flag.
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ColorsArg {
    /// 24-bit if `COLORTERM` says so, else 256 colours; no colour under `NO_COLOR`.
    Auto,
    Truecolor,
    #[value(name = "256")]
    Ansi256,
    None,
}

impl ColorsArg {
    /// The [`ColorMode`] this flag selects.
    #[must_use]
    pub fn resolve(self) -> ColorMode {
        match self {
            Self::Auto => ColorMode::from_environment(),
            Self::Truecolor => ColorMode::Truecolor,
            Self::Ansi256 => ColorMode::Ansi256,
            Self::None => ColorMode::None,
        }
    }
}

/// overflight's command-line arguments.
#[derive(Debug, Parser)]
#[command(
    name = "overflight",
    version,
    about = "See the aircraft flying above you as a live sky view.",
    long_about = LONG_ABOUT,
    after_help = AFTER_HELP
)]
pub struct Cli {
    /// Observer latitude in decimal degrees.
    #[arg(long, allow_negative_numbers = true, value_name = "DEGREES")]
    pub lat: Option<f64>,

    /// Observer longitude in decimal degrees.
    #[arg(long, allow_negative_numbers = true, value_name = "DEGREES")]
    pub lon: Option<f64>,

    /// Observer height above sea level, in metres.
    #[arg(long, allow_negative_numbers = true, value_name = "METRES")]
    pub alt: Option<f64>,

    /// Where the aircraft data comes from [default: adsb-lol].
    #[arg(long, value_enum, value_name = "SOURCE")]
    pub source: Option<SourceArg>,

    /// URL of a local receiver's aircraft.json (with --source local).
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    /// Replay recorded traffic from the bundled fixture; no network needed.
    #[arg(long)]
    pub demo: bool,

    /// How far out to look, in kilometres [default: 80].
    #[arg(long, allow_negative_numbers = true, value_name = "KM")]
    pub radius_km: Option<f64>,

    /// Ignore aircraft below this elevation, in degrees [default: 0].
    #[arg(long, allow_negative_numbers = true, value_name = "DEGREES")]
    pub min_elevation: Option<f64>,

    /// Seconds between updates, no faster than the source allows.
    #[arg(long, value_name = "SECONDS")]
    pub interval: Option<u64>,

    /// Screensaver mode: any key exits.
    #[arg(long)]
    pub screensaver: bool,

    /// Append a CSV row per aircraft when it leaves the sky or you quit.
    #[arg(long, value_name = "PATH")]
    pub log: Option<PathBuf>,

    /// Do not capture the mouse, so you can select text as usual.
    #[arg(long)]
    pub no_mouse: bool,

    /// Do not look up the selected aircraft's route (no adsbdb request).
    #[arg(long)]
    pub no_routes: bool,

    /// Ring the terminal bell when an unusual aircraft appears.
    #[arg(long)]
    pub bell: bool,

    /// Colour support: auto, truecolor, 256 or none [default: auto].
    #[arg(long, value_enum, value_name = "MODE")]
    pub colors: Option<ColorsArg>,

    /// Fetch once, print a table of aircraft, then exit.
    #[arg(long, hide = true)]
    pub dump: bool,

    /// Directory to save raw responses into, for building fixtures.
    #[arg(long, hide = true, value_name = "DIR")]
    pub record: Option<PathBuf>,

    /// Fake the current time as an RFC 3339 timestamp, for checking palettes.
    #[arg(long, hide = true, value_name = "RFC3339")]
    pub time: Option<String>,

    /// Celestrak group to draw satellites from.
    #[arg(long, hide = true, value_name = "GROUP")]
    pub tle_group: Option<String>,
}
