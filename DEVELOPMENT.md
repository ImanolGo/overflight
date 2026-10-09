# Development notes

This document records how overflight was built and how it is released. The plan
it followed lives in [PLAN.md](PLAN.md); this file is the after-the-fact log.

## Ground rules

Every commit is meant to stand on its own:

```sh
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
```

all pass at each stage. Tests never touch the network: every data source sits
behind the `Provider` trait, and the tests use the fixture provider and inline
JSON. `unwrap`/`expect` are confined to tests and `main` setup.

## The milestones

| Stage | Commit subject | What it delivers |
| --- | --- | --- |
| M0 | project skeleton | Crate, dependencies, CI on Linux/macOS/Windows, and a horizon circle with compass letters that quits on `q`. |
| M1 | geodesy and solar position | WGS84 ECEF→ENU, azimuth/elevation/range, the polar projection, and the NOAA solar elevation. |
| M2 | data providers | `Aircraft`/`Query`/`Provider`, the shared readsb parser, airplanes.live, local, OpenSky and fixture providers, and a hidden `--dump`. |
| M3 | live sky | Fetcher thread, dead reckoning with easing, trails, arrows, labels and the status line, plus a stable snapshot test. |
| M4 | sky colours and stars | Day/twilight/night palettes driven by the sun, a fixed star field, and a hidden `--time`. |
| M5 | interaction and config | `Tab`/`l`/`t`/`m`/`u`, the detail box, `--screensaver`/`--min-elevation`/`--interval`, and the config file. |
| M6 | polish and release | Empty-sky hint, docs, the vhs demo GIF, `CHANGELOG.md`, and the release tooling. |

## Notable decisions and deviations

- **The crate is a library plus a thin binary.** The reusable pieces (geometry,
  providers, tracking, rendering) live in `lib.rs` so they can be tested and
  documented; `main.rs` owns the terminal and the CLI. This also lets docs.rs
  build.
- **airplanes.live now limits its API to its own feeders.** It answers every
  other request with `403 Please contact us...`, so it cannot be the default for
  a new user. **adsb.lol**, which serves the identical readsb JSON shape and is
  open to everyone, is the default source; airplanes.live stays selectable with
  `--source airplanes-live`. The bundled demo fixture was recorded from adsb.lol
  (60 s at 5 s intervals around Heathrow).
- **`--demo` ignores the config file's location.** The fixture was recorded
  around one place, so it always uses that observer; otherwise a real config
  made every recorded plane appear far away and below the horizon.
- **The sky paints its own background.** Rather than tinting against the
  terminal's theme (which we cannot detect reliably), each palette fills the
  sky area, so day/twilight/night look the same on a light or a dark terminal.
- **Imperial units mean feet, knots and miles** (aviation convention); metric is
  metres, km/h and kilometres.
- **Frame rate vs. CPU.** The sky redraws at 30 fps while aircraft are moving
  and at 4 fps when it is empty. Measured CPU stays around 1% of a core with
  traffic (under the plan's 2% target) and lower when idle.
- **Cross-platform azimuth.** A due-north target can come out as `360.0` rather
  than `0.0` depending on the sign of a floating-point rounding error, so
  azimuth comparisons are wrap-aware.

## After 0.1

The ideas PLAN.md listed for after 0.1 were built, one commit each:

- **Satellites and the ISS.** `satellite.rs` parses TLEs and propagates them
  with SGP4 (the `sgp4` crate), rotating TEME → ECEF → ENU. TLEs are refreshed
  from Celestrak in the background, and a recorded set is embedded for offline
  `--demo`.
- **Moon and bright planets.** `sky.rs` computes low-precision geocentric
  positions (Schlyter's method) for the Moon and the planets, cross-checked
  against `sun.rs` and the 2024-04-08 solar eclipse.
- **Emitter-category glyphs.** Helicopters, gliders and balloons are drawn with
  their own symbol, from the ADS-B emitter category.
- **Unusual-aircraft notifications.** Military aircraft, emergency squawks and
  a few rare types are highlighted and raise a transient banner.
- **Horizon mode.** `h` switches to a side-on view looking north, with a
  skyline and planes rising over it.

A later pass (0.3.0 and the 0.4.0 features) added:

- **Bug fixes from review.** Quitting no longer hangs; TLEs are cached with
  12-hour refresh and a 24-hour backoff; the observer's altitude is used;
  location/radius are validated; rate limits are honoured; labels no longer
  flicker or overlap; rare types are corrected.
- **Real stars.** `sky.rs` embeds an extract of the Yale Bright Star Catalogue
  and converts it with the same `horizontal()`; `c` toggles constellation lines.
- **Coming-overhead prediction.** `Track::next_peak` dead-reckons five minutes
  ahead and the status line shows the best pass.
- **ISS pass alerts.** `satellite::next_visible_pass` finds the next pass that
  is sunlit with a dark sky; shown at twilight and announced when it starts.
- **Spotter's logbook** (`--log`), **route lookup** (adsbdb), **click to
  select** (`--no-mouse`), and an opt-in **bell** (`--bell`).

## Releasing

A release is just a version tag:

1. Bump `version` in `Cargo.toml`, add a `CHANGELOG.md` entry, and commit.
2. `git tag -a vX.Y.Z -m "overflight X.Y.Z"` and `git push origin vX.Y.Z`.

The tag drives the rest:

- **Release** (`dist`, `.github/workflows/release.yml`) builds the Linux/macOS/
  Windows archives with checksums and the shell/PowerShell/MSI installers, and
  creates the GitHub Release. Configuration lives in `dist-workspace.toml`.
- **Debian package** (`.github/workflows/deb.yml`) builds the `.deb` with
  `cargo-deb` and attaches it once the release appears.
- **Publish to crates.io** (`.github/workflows/publish.yml`) publishes the crate
  through Trusted Publishing (OIDC). The one-time crates.io setup is documented
  at the top of that workflow.
- **CI** (`.github/workflows/ci.yml`) runs fmt/clippy/tests on every push, and a
  pinned job builds at the declared MSRV (`rust-version`).

docs.rs builds once the crate is on crates.io.

## Running it

```sh
cargo run -- --demo            # recorded traffic, no network
cargo run -- --dump --demo     # one snapshot as a table, then exit
cargo run -- --lat 52.52 --lon 13.40
```
