# Changelog

All notable changes to `sgp4-predict-cli` (the command-line interface) are
documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Release automation extracts the section whose heading matches the crate version
in the release PR, so keep each version's notes under a `## [x.y.z] - YYYY-MM-DD`
heading. Add work-in-progress notes under `## [Unreleased]`.

## [Unreleased]

## [0.1.0] - 2026-07-25

Initial release — a command-line front-end to `sgp4-predict`.

- Five subcommands: `observations`, `transits`, `state-vectors`, `apsides`, and
  `illumination`, over a configurable start time and duration.
- TLE input from a file (2- or 3-line) or an interactive prompt; ground location given as
  `--observer "lat,lon,alt"` or prompted.
- Tabular output to stdout or a file, with optional argument echoing and stale-TLE warnings.
