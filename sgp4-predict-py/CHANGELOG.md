# Changelog

All notable changes to the `sgp4-predict` Python package (the PyO3 bindings) are
documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Record unreleased work under the Unreleased heading below. The release
automation rolls that section into a dated `## [x.y.z] - YYYY-MM-DD` heading and
publishes it verbatim as the GitHub Release body — see `docs/RELEASING.md`.

## [Unreleased]

## [0.2.0] - 2026-08-24

### Added

- Detection tuning as keyword-only arguments on `transits_iter`, `aoi_iter`, `apsis_iter`,
  `illumination_iter`, `detect_transit`, `detect_aoi` and `max_elevation` — scan and walk steps, the
  window-duration caps, and how partial windows at the interval edges are treated. Anything left
  unset keeps the library's default.
- `Refinement(time_tolerance=…, max_iter=…)` — keyword-only, each defaulting to the library's value;
  the class previously took no arguments, so both fields had to be assigned after construction.
- Area-of-interest detection: `Predictor.aoi_iter` and `detect_aoi` yield the `AoiWindow`s during
  which an area is within the payload's reach — see `max_off_nadir_deg` and `coverage`.
  `Polygon`, `Rectangle` and `Circle` describe the
  region; each also exposes `signed_angular_offset_deg`. Points are `LatLon` objects, `Geodetic`
  objects whose altitude is ignored, or plain `(latitude_deg, longitude_deg)` tuples, and
  `AoiWindow` satisfies `IntervalRange`.
- `LatLon` and `Geodetic` types, `Predictor.sub_point` — the geodetic point directly beneath the
  satellite — and `Predictor.ground_track_iter`, sampling those points at a fixed cadence.
- Value equality (`==`) on the result and input types — `Transit`, `AoiWindow`, `Illumination`,
  `Observation`, `Apsis`, `Tle`, `Refinement`, `Vec3` and the state vectors. `Transit`,
  `AoiWindow`, `Illumination` and `Tle` are also hashable, so they work as `dict` keys and in
  `set`s.

### Fixed

- Wheels ship a `py.typed` marker. Without it PEP 561 tells type checkers to ignore the bundled
  stubs, so the package was effectively untyped despite advertising `Typing :: Typed`.
- Wheels ship the generated stub. It was excluded from the repository and the release build does
  not regenerate it, so every class other than the handful redeclared by hand — `Transit`,
  `Observation`, `Apsis`, `Tle`, `GroundObserver`, `Vec3`, the state vectors and all the iterators
  — arrived with no type information at all.
- Iterating any iterator yields its item type rather than `T | None`. `__next__` was declared
  `Optional`, so `for transit in predictor.transits_iter(...)` bound `Transit | None` and every
  subsequent attribute access was reported as an error.
- Docstrings reach the editor. `Predictor`, `Elements`, `Polygon`, `Rectangle` and `Circle` were
  redeclared in the hand-written stub to narrow argument types, which replaced the generated
  declarations and dropped every method docstring with them. The argument types are now narrowed at
  the source instead, so the documentation and the types arrive together.

## [0.1.0] - 2026-07-28

Initial release — Python bindings (PyO3) for `sgp4-predict`.

- `Predictor` constructed from a TLE or OMM (`Elements` via JSON/dict): propagate state
  vectors and compute observations from a `GroundObserver`.
- Transit, observation, apsis, and illumination iterators plus one-shot helpers
  (`detect_transit`, `max_elevation`, `illumination_state`), working with native
  `datetime` / `timedelta`; angles as plain floats (`_deg`-suffixed).
- Prebuilt `abi3` wheels for CPython ≥ 3.10 on Linux, macOS, and Windows, with type stubs.
