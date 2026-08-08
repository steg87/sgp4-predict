# Changelog

All notable changes to the `sgp4-predict` Python package (the PyO3 bindings) are
documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Record unreleased work under the Unreleased heading below. The release
automation rolls that section into a dated `## [x.y.z] - YYYY-MM-DD` heading and
publishes it verbatim as the GitHub Release body — see `docs/RELEASING.md`.

## [Unreleased]

### Added

- Area-of-interest detection: `Predictor.aoi_iter` and `detect_aoi` yield the `AoiWindow`s during
  which the sub-satellite point is inside an area. `Polygon`, `Rectangle` and `Ellipse` describe the
  region; each also exposes `signed_angular_offset_deg`. Points are `LatLon` objects, `Geodetic`
  objects whose altitude is ignored, or plain `(latitude_deg, longitude_deg)` tuples, and
  `AoiWindow` satisfies `IntervalRange`.
- `LatLon` and `Geodetic` types, `Predictor.sub_point` — the geodetic point directly beneath the
  satellite — and `Predictor.ground_track_iter`, sampling those points at a fixed cadence.
- Value equality (`==`) on the result and input types — `Transit`, `AoiWindow`, `Illumination`,
  `Observation`, `Apsis`, `Tle`, `Refinement`, `Vec3` and the state vectors. `Transit`,
  `AoiWindow`, `Illumination` and `Tle` are also hashable, so they work as `dict` keys and in
  `set`s.

## [0.1.0] - 2026-07-28

Initial release — Python bindings (PyO3) for `sgp4-predict`.

- `Predictor` constructed from a TLE or OMM (`Elements` via JSON/dict): propagate state
  vectors and compute observations from a `GroundObserver`.
- Transit, observation, apsis, and illumination iterators plus one-shot helpers
  (`detect_transit`, `max_elevation`, `illumination_state`), working with native
  `datetime` / `timedelta`; angles as plain floats (`_deg`-suffixed).
- Prebuilt `abi3` wheels for CPython ≥ 3.10 on Linux, macOS, and Windows, with type stubs.
