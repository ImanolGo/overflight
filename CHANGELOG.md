# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-10-09

The first release. A live, overhead view of the aircraft above you.

### Added

- Live sky view: a horizon circle with 30°/60° rings and compass points, where
  every aircraft drifts in real time with its callsign, a heading arrow, a
  trailing dotted path, and colour by altitude band.
- Dead reckoning between polls, easing smoothly onto each fresh position, with
  a 60-second trail and a fade-out before aircraft are dropped.
- Day, twilight and night palettes chosen from the sun's elevation, with a
  fixed star field in the night sky.
- Data sources: airplanes.live (default), a local readsb/dump1090 receiver over
  HTTP, and OpenSky via OAuth2, plus a `--demo` mode that replays a recorded
  fixture with no network.
- A hidden `--dump` mode that fetches once and prints a table of nearby
  aircraft with azimuth, elevation and distance.
- Keyboard controls: `Tab` to select and show a detail box, and `l` / `t` / `m`
  / `u` to toggle callsigns, trails, sky/map orientation and units.
- A config file at `~/.config/overflight/config.toml` (see
  `overflight.example.toml`); command-line flags take precedence.
- Flags for `--lat`/`--lon`, `--radius-km`, `--min-elevation`, `--interval`,
  `--source`, `--url`, `--demo` and `--screensaver`.
- Hidden development flags: `--record` to capture raw responses into fixtures,
  and `--time` to fake the clock for checking the palettes.

[0.1.0]: https://github.com/ImanolGo/overflight/releases/tag/v0.1.0
