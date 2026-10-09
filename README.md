# overflight

[![crates.io](https://img.shields.io/crates/v/overflight.svg?v=2)](https://crates.io/crates/overflight)
[![docs.rs](https://img.shields.io/docsrs/overflight/latest?v=3)](https://docs.rs/overflight)
[![CI](https://github.com/ImanolGo/overflight/actions/workflows/ci.yml/badge.svg)](https://github.com/ImanolGo/overflight/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/ImanolGo/overflight?v=2)](https://github.com/ImanolGo/overflight/releases)
[![License: MIT](https://img.shields.io/crates/l/overflight.svg?v=2)](https://github.com/ImanolGo/overflight/blob/main/LICENSE)

Look up from your terminal.

![overflight: aircraft drifting across a starry night sky, with a detail box for the selected plane](https://raw.githubusercontent.com/ImanolGo/overflight/main/demo.gif)

> **Status:** 0.2 — usable and still growing. The plan lives in [PLAN.md](https://github.com/ImanolGo/overflight/blob/main/PLAN.md); see [DEVELOPMENT.md](https://github.com/ImanolGo/overflight/blob/main/DEVELOPMENT.md) for how it is built and released.

overflight shows the aircraft flying above you right now, drawn as if you were lying on your back in a field and looking straight up. The edge of the circle is the horizon, the middle is directly overhead, and every plane drifts across it in real time with its callsign trailing behind. During the day the sky is blue, at dusk it fades, and at night you get stars.

It's a screensaver, mostly. But it's also the quickest way to answer "what was that plane that just went over?"

## Reading the sky

- **Centre** is straight up. **Edge** is the horizon. The faint rings mark 30° and 60° above the horizon.
- The arrow on each aircraft shows which way it's heading, and the dotted trail shows where it's been in the last minute.
- Colour is by altitude: warm for low aircraft, cool for cruise, neutral in between. A `+` or `-` after the callsign means climbing or descending.
- Planes higher in the sky are closer to you. A plane near the edge is far away, low on the horizon, or both.
- Like a star chart, east and west are swapped compared to a map. That's what the sky looks like when you face up with north at the top of your head. Press `m` if you'd rather have it the map way round.
- Helicopters, gliders and balloons are drawn with their own symbol rather than a heading arrow. Military aircraft, emergency squawks and a few rare types are highlighted, with a banner when one appears.
- The Moon is drawn whenever it is above the horizon, even in daylight; the bright planets appear at twilight and night, and satellites — including the ISS — drift across as slow-moving dots.

Press `h` for a side-on view instead: a skyline looking north, with planes rising over it.

The horizon, rings and trails are drawn with Braille characters, which virtually
every terminal font includes.

## Install

Prebuilt binaries for Linux, macOS and Windows — plus a `.deb` and an MSI — are
on the [releases page](https://github.com/ImanolGo/overflight/releases).

Or, with a recent stable Rust toolchain:

```sh
cargo install overflight                                    # from crates.io
cargo install --git https://github.com/ImanolGo/overflight   # latest from git
cargo install --path .                                      # from a checkout
```

## Usage

Tell it where you are:

```sh
overflight --lat 52.52 --lon 13.40
```

or put your location in `~/.config/overflight/config.toml` (see the
[`overflight.example.toml`](https://github.com/ImanolGo/overflight/blob/main/overflight.example.toml)
for that path on macOS and Windows) so you don't have to type it every time:

```toml
lat = 52.52
lon = 13.40
alt_m = 0          # how high you are, in metres
radius_km = 80     # how far out to look
units = "metric"   # or "imperial"
min_elevation = 0  # ignore planes lower than this
```

Command-line flags always win over the config file.

Other things you can do:

```sh
overflight --demo              # recorded traffic, no network needed
overflight --screensaver       # any key exits
overflight --min-elevation 10  # ignore planes that are low on the horizon
overflight --radius-km 40      # look closer to home
overflight --alt 1650          # how high you are, for low, close traffic
overflight --interval 10       # seconds between updates
overflight --log ~/flights.csv # log each aircraft when it leaves the sky
overflight --source local --url http://your-pi/data/aircraft.json
overflight --source opensky    # uses OPENSKY_CLIENT_ID / OPENSKY_CLIENT_SECRET
```

Keys while it's running:

| Key | Does |
| --- | --- |
| `q` / `Esc` | Quit |
| `Tab` | Cycle through aircraft and show details |
| `l` | Show or hide callsigns |
| `t` | Show or hide trails |
| `m` | Switch between sky view and map orientation |
| `h` | Switch to the side-on horizon view |
| `←` / `→` | Turn the horizon view |
| `c` | Show or hide constellation lines |
| `u` | Switch between metric and imperial |

The detail box for the selected aircraft shows its callsign, registration,
aircraft type, altitude, ground speed, distance from you, and where to look
("north-east, 38° up").

`units = "metric"` shows metres, km/h and kilometres; `"imperial"` shows feet,
knots and miles.

## Where the data comes from

By default overflight uses the free [airplanes.live](https://airplanes.live) API, which is run by volunteers who feed ADS-B data from receivers all over the world. It's free for non-commercial use, and overflight stays within its rate limit by asking for an update every few seconds and filling in the gaps by estimating where each plane has moved since. (Some networks block airplanes.live's API; if you see a 403, try `--demo` or one of the other sources.)

If you have an [OpenSky Network](https://opensky-network.org) account you can use that instead with `--source opensky` and your OAuth2 client credentials (`client_id` and `client_secret`, created on your account page). Put them in the config file or the `OPENSKY_CLIENT_ID` / `OPENSKY_CLIENT_SECRET` environment variables; they're never logged. Anonymous OpenSky access has a small daily quota, so it isn't a good fit for something that runs all day.

If you run your own ADS-B receiver (a Raspberry Pi with a cheap SDR dongle and readsb or dump1090), point overflight at it with `--source local --url http://your-pi/data/aircraft.json`. That's the nicest setup: no rate limits, no internet needed, and you see exactly what your antenna sees.

Satellites come from Two-Line Elements: overflight fetches the current set from [Celestrak](https://celestrak.org) (the `stations` group, which includes the ISS) and propagates them with SGP4. The Moon and the bright planets are computed locally, with no network needed.

`overflight --demo` replays a minute of recorded traffic (captured around Heathrow) from the bundled fixture, so you can try it with no receiver and no network at all. It also uses a recorded set of TLEs.

Coverage depends on volunteer receivers, so some areas, especially rural ones and over the sea, will be quieter than reality.

## Privacy

Your coordinates are only ever sent to the data source you pick, as part of the request for nearby aircraft. They're not logged or sent anywhere else. If you share screenshots, remember that the planes around you give a fair idea of where you live.

## Contributing

Issues and pull requests are welcome. Ideas I'd love help with: satellites (the ISS passing over would look great), helicopters drawn differently from airliners, and a sound when something interesting flies over.

## License

MIT, see [LICENSE](https://github.com/ImanolGo/overflight/blob/main/LICENSE).
