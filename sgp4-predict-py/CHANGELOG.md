# Changelog

All notable changes to the `sgp4-predict` Python package (the PyO3 bindings) are
documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Release automation extracts the section whose heading matches the crate version
in the release PR, so keep each version's notes under a `## [x.y.z] - YYYY-MM-DD`
heading. Add work-in-progress notes under `## [Unreleased]`.

## [Unreleased]

## [0.1.0] - 2026-07-25

Initial release — Python bindings (PyO3) for `sgp4-predict`.

- `Predictor` constructed from a TLE or OMM (`Elements` via JSON/dict): propagate state
  vectors and compute observations from a `GroundObserver`.
- Transit, observation, apsis, and illumination iterators plus one-shot helpers
  (`detect_transit`, `max_elevation`, `illumination_state`), working with native
  `datetime` / `timedelta`; angles as plain floats (`_deg`-suffixed).
- Prebuilt `abi3` wheels for CPython ≥ 3.10 on Linux, macOS, and Windows, with type stubs.
