# overflight

Look up from your terminal.

overflight shows the aircraft flying above you right now, drawn as if you were lying on your back in a field and looking straight up. The edge of the circle is the horizon, the middle is directly overhead, and every plane drifts across it in real time with its callsign trailing behind. During the day the sky is blue, at dusk it fades, and at night you get stars.

It's a screensaver, mostly. But it's also the quickest way to answer "what was that plane that just went over?"

> **Status:** early days. The plan lives in [PLAN.md](PLAN.md) and things will move around until 0.1.

```
                         N
              .  ·    ·      ·     .
         ·       DLH4AB ↗            ·
       ·        ··                      ·
      ·                                  ·
     ·      ·            ·   EZY82QP      ·
   E ·                 +              ←···   W
     ·                                    ·
      ·        ↓ RYR5HK                  ·
       ·        ·                       ·
         ·                            ·
              .  ·    ·      ·     .
                         S
```

## Reading the sky

- **Centre** is straight up. **Edge** is the horizon. The faint rings mark 30° and 60° above the horizon.
- The arrow on each aircraft shows which way it's heading, and the dotted trail shows where it's been in the last minute.
- Planes higher in the sky are closer to you. A plane near the edge is far away, low on the horizon, or both.
- Like a star chart, east and west are swapped compared to a map. That's what the sky looks like when you face up with north at the top of your head. Press `m` if you'd rather have it the map way round.

## Install

You'll need a recent stable Rust toolchain.

```sh
cargo install --git https://github.com/<you>/overflight
```

## Usage

Tell it where you are:

```sh
overflight --lat 52.52 --lon 13.40
```

or put your location in `~/.config/overflight/config.toml` so you don't have to type it every time:

```toml
lat = 52.52
lon = 13.40
radius_km = 80     # how far out to look
units = "metric"   # or "imperial"
```

Other things you can do:

```sh
overflight --screensaver      # any key exits
overflight --min-elevation 10 # ignore planes that are low on the horizon
overflight --demo             # recorded traffic, no network needed
```

Keys while it's running:

| Key | Does |
| --- | --- |
| `q` / `Esc` | Quit |
| `Tab` | Cycle through aircraft and show details |
| `l` | Show or hide callsigns |
| `t` | Show or hide trails |
| `m` | Switch between sky view and map orientation |
| `u` | Switch between metric and imperial |

The detail box for the selected aircraft shows its callsign, registration, aircraft type, altitude, ground speed, distance from you, and where to look ("north-east, 38° up").

## Where the data comes from

By default overflight uses the free [airplanes.live](https://airplanes.live) API, which is run by volunteers who feed ADS-B data from receivers all over the world. It's free for non-commercial use, and overflight stays well under its rate limit by asking for an update every few seconds and filling in the gaps by estimating where each plane has moved since.

If you have an [OpenSky Network](https://opensky-network.org) account you can use that instead with `--source opensky` and your API client credentials. Anonymous OpenSky access has a small daily quota, so it isn't a good fit for something that runs all day.

If you run your own ADS-B receiver (a Raspberry Pi with a cheap SDR dongle and readsb or dump1090), point overflight at it with `--source local --url http://your-pi/data/aircraft.json`. That's the nicest setup: no rate limits, no internet needed, and you see exactly what your antenna sees.

Coverage depends on volunteer receivers, so some areas, especially rural ones and over the sea, will be quieter than reality.

## Privacy

Your coordinates are only ever sent to the data source you pick, as part of the request for nearby aircraft. They're not logged or sent anywhere else. If you share screenshots, remember that the planes around you give a fair idea of where you live.

## Contributing

Issues and pull requests are welcome. Ideas I'd love help with: satellites (the ISS passing over would look great), helicopters drawn differently from airliners, and a sound when something interesting flies over.

## License

MIT, see [LICENSE](LICENSE).
