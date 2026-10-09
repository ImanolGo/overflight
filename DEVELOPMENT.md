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
- **airplanes.live is unreachable from some networks.** It answered every
  request from the development environment with `403 Please contact us...`, so
  the default source could not be exercised live there. The bundled demo
  fixture was therefore recorded from **adsb.lol**, which serves the identical
  readsb JSON shape, for 60 s at 5 s intervals around Heathrow.
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

docs.rs builds once the crate is on crates.io. The AUR `PKGBUILD` in
`packaging/aur` is updated by hand (bump `pkgver`, refresh the hash with
`updpkgsums`, regenerate `.SRCINFO`).

## Running it

```sh
cargo run -- --demo            # recorded traffic, no network
cargo run -- --dump --demo     # one snapshot as a table, then exit
cargo run -- --lat 52.52 --lon 13.40
```
