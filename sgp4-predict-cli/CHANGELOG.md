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
- `aoi-windows`, finding the windows in which an area of interest is within the payload's reach,
  reporting the sub-satellite point at each boundary crossing. The AOI is named by `--aoi <id>`.
  `--max-off-nadir <deg>` is the half-angle of the satellite's field of regard, defaulting to 0 —
  the ground track itself crossing the area — and `--coverage full` requires every part of the area
  to be in reach at once rather than any part of it.
- An `aois:` map in the config file, alongside `groundstations:`. Each AOI is tagged with its
  `shape` — `box`, `circle`, or `polygon` — and given as named fields; everything is in
  degrees. A `box` is its four bounds (`south`, `north`, `west`, `east`), running eastward from
  `west` so that an `east` at a smaller longitude wraps the antimeridian. A `circle` is its centre
  and `radius`.
- `aoi add|remove|list` (aliases `rm`, `ls`) to manage those AOIs. `aoi add` prompts field by field
  like `gs add`, accepting each shape name's initial (`b`/`c`/`p`), and reads polygon vertices
  one per line until a blank line. The id and `--shape` may be given as arguments; coordinates never
  are. Geometry the library cannot build is refused before anything is written.
- `gs add` accepts the station id as an argument, and `-f` / `--force` to replace an existing
  station rather than erroring. An id or shape supplied that way is echoed as
  though it had been typed, so the transcript reads the same either way.
- Prompts re-ask on a malformed line instead of aborting, so a typo no longer discards the fields
  already entered. This applies to `gs add` too.
- Detection tuning flags on `transits`, `apsides`, `illumination` and `aoi-windows`, exposing the
  library's `*Opts` and `Refinement` knobs with their existing defaults, so an ordinary run is
  unchanged. Which flags a subcommand takes depends on how it scans: `--step` where the stride is
  fixed, `--min-step` / `--max-step` where it adapts, plus `--walk-step`, `--max-transit-duration`
  or `--max-window-duration`, `--skip-leading-partial`, `--clamp-to-interval`, `--tca-scan-step`,
  `--time-tolerance` and `--max-iter`. See each subcommand's `--help`.
  `--max-window-duration` in particular was previously unreachable, so a continental-scale AOI
  could not be scanned at all.

### Changed

- `groundstations:` is omitted from a saved config when empty, so a config holding only AOIs no
  longer grows an empty stub.
- `--output-args` records every resolved tuning knob, and each line is spelled as the flag that
  sets it, so a recorded header can be pasted back onto the command line.

### Fixed

- `PolygonDef` rejects unknown fields, like every other config struct. A typo alongside a valid
  `vertices` list was silently dropped.
- Prompts reject `nan` and `inf`. Both parse as numbers but pass every range check, so a non-finite
  West longitude made the East prompt unsatisfiable and a non-finite centre only failed once every
  field had been entered.
- A detection-tuning flag rejected during conversion no longer leaves an empty `--out` file behind.

### Removed

- The `--output-args` header no longer emits `tle-source`, `config`, `format` or `out`. `format` was
  always `text`, `out` named the file the line was written into, and the other two were local paths
  whose content is already recorded literally on the surrounding lines.

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
