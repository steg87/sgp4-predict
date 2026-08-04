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
  reporting the sub-satellite point at each boundary crossing. The AOI is named by `--aoi <id>`.
- An `aois:` map in the config file, alongside `groundstations:`. Each AOI is tagged with its
  `shape` — `box`, `ellipse`, `circle`, or `polygon` — and given as named fields; everything is in
  degrees. A `box` is its four bounds (`south`, `north`, `west`, `east`), running eastward from
  `west` so that an `east` at a smaller longitude wraps the antimeridian.
- `aoi add|remove|list` (aliases `rm`, `ls`) to manage those AOIs. `aoi add` prompts field by field
  like `gs add`, accepting each shape name's initial (`b`/`e`/`c`/`p`), and reads polygon vertices
  one per line until a blank line. The id and `--shape` may be given as arguments; coordinates never
  are. Geometry the library cannot build is refused before anything is written.
- `gs add` accepts the station id as an argument, and `-f` / `--force` to replace an existing
  station rather than erroring. An id or shape supplied that way is echoed as
  though it had been typed, so the transcript reads the same either way.
- Prompts re-ask on a malformed line instead of aborting, so a typo no longer discards the fields
  already entered. This applies to `gs add` too.

### Changed

- `groundstations:` is omitted from a saved config when empty, so a config holding only AOIs no
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
