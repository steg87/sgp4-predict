# Changelog

All notable changes to `sgp4-predict-cli` (the command-line interface) are
documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Record unreleased work under the Unreleased heading below. The release
automation rolls that section into a dated `## [x.y.z] - YYYY-MM-DD` heading and
publishes it verbatim as the GitHub Release body — see `docs/RELEASING.md`.

## [Unreleased]

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
