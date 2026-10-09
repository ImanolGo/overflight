# Changelog

All notable changes to this project are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-10-09

### Added

- Satellites, including the ISS: TLEs are fetched from Celestrak and
  propagated with SGP4, drawn as slow-moving dots at night. A recorded set is
  bundled so `--demo` works offline.
- The Moon and the bright planets in the night sky, drawn with their
  astronomical symbols.
- Distinct glyphs for helicopters, gliders and balloons, from the ADS-B
  emitter category.
- A notification banner and alert colour for unusual aircraft: military,
  emergency squawks, and a few rare types.
- Horizon mode (`h`): a side-on view looking north, with planes rising over a
  skyline.

[0.2.0]: https://github.com/ImanolGo/overflight/releases/tag/v0.2.0

## [0.1.2] - 2026-10-09

### Changed

- Documentation only. The README now leads with prebuilt binaries and
  `cargo install overflight`, uses absolute links so it renders correctly on
  crates.io and docs.rs, and notes that the sky is drawn with Braille.

[0.1.2]: https://github.com/ImanolGo/overflight/releases/tag/v0.1.2

## [0.1.1] - 2026-10-09

### Added

- Release automation: cargo-dist builds Linux/macOS/Windows archives and
  shell/PowerShell/MSI installers and creates the GitHub Release; a Debian
  package is attached; and the crate publishes to crates.io through Trusted
  Publishing.
- A pinned MSRV CI job, README badges, and `DEVELOPMENT.md`.

### Fixed

- `--demo` ignores the config file's location, so a real config no longer makes
  every recorded aircraft appear far away and below the horizon.
- An empty or failed sky now shows a centred hint instead of a bare sky.

[0.1.1]: https://github.com/ImanolGo/overflight/releases/tag/v0.1.1

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
