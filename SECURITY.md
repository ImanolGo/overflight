# Security policy

overflight talks to public ADS-B APIs (adsb.lol by default, optionally
airplanes.live, OpenSky or your own receiver), to Celestrak for satellite
elements, and to adsbdb for route lookups. It reads and writes your config file
and, with `--log`, a CSV file you name. It is a terminal program with no server
component.

## Supported versions

The latest release is the supported one. Fixes for anything security-related go
into a patch release on the current minor version; older versions are not
maintained.

## Reporting a vulnerability

Please report suspected vulnerabilities privately, using GitHub's
[private vulnerability reporting](https://docs.github.com/en/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)
on the repository's **Security** tab, rather than in a public issue.

Useful details: the version (`overflight --version`), your OS and terminal, the
data source in use, and steps to reproduce. We aim to acknowledge reports within
a week and will credit reporters in the release notes unless asked not to.
