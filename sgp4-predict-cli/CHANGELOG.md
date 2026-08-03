# Changelog

All notable changes to `sgp4-predict-cli` (the command-line interface) are
documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Record unreleased work under the Unreleased heading below. The release
automation rolls that section into a dated `## [x.y.z] - YYYY-MM-DD` heading and
publishes it verbatim as the GitHub Release body — see `docs/RELEASING.md`.

## [Unreleased]

### Added

- `ground-track`, sampling the geodetic point directly beneath the satellite at `--step`.
- `aoi-windows`, finding the windows in which the ground track lies inside an area of interest,
  reporting the sub-satellite point at each boundary crossing. The area is named by `--area <id>`.
- An `areas:` map in the config file, alongside `groundstations:`. Each area is tagged with its
  `shape` — `box`, `ellipse`, `circle`, or `polygon` — and given as named fields; all extents are
  in degrees of arc.
- `aoi add|remove|list` (aliases `rm`, `ls`) to manage those areas. `aoi add` prompts field by field
  like `gs add`, accepting each shape name's initial (`b`/`e`/`c`/`p`), and reads polygon vertices
  one per line until a blank line. Anything given up front is not prompted for: the id is
  positional, and the shape may be supplied as exactly one of `--box LAT,LON,W,H`,
  `--ellipse LAT,LON,A,B[,BEARING]`, `--circle LAT,LON,R`, or `--poly "(LAT,LON),(LAT,LON),..."`.
  Geometry the library cannot build is refused before anything is written.
- Prompts re-ask on a malformed line instead of aborting, so a typo no longer discards the fields
  already entered. This applies to `gs add` too.

### Changed

- `groundstations:` is omitted from a saved config when empty, so a config holding only areas no
  longer grows an empty stub.

## [0.1.0] - 2026-07-28

Initial release — a command-line front-end to `sgp4-predict`.

- Five prediction subcommands: `observations`, `transits`, `state-vectors`, `apsides`, and
  `illumination`, over a configurable start time and duration.
- TLE input from a file (2- or 3-line) or stdin, so TLEs can be piped straight in.
- Ground stations defined in a YAML config file and selected with `--gs <id>`, managed by hand or
  through `gs add` / `gs list` / `gs remove`.
- Text, newline-delimited JSON, and CSV output to stdout or a file, with optional `--output-args`
  echoing of the resolved inputs and stale-TLE warnings on stderr.
- Shell completions and a man page generated from the argument definitions.
