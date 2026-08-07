# Changelog

All notable changes to `sgp4-predict` (the library crate) are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Record unreleased work under the Unreleased heading below. The release
automation rolls that section into a dated `## [x.y.z] - YYYY-MM-DD` heading and
publishes it verbatim as the GitHub Release body — see `docs/RELEASING.md`.

## [Unreleased]

### Added

- Area-of-interest detection: `Predictor::aoi_iter` and `detect_aoi` yield the `AoiWindow`s during
  which the sub-satellite point is inside an `Area`. `Polygon` describes an arbitrary ring of
  latitude/longitude vertices — concave and self-intersecting rings are supported, with
  `FillRule::NonZero` (default) or `FillRule::EvenOdd` deciding the interior of the latter. The ring
  closes implicitly and vertex order does not matter. Implement `Area` for other shapes.
- `Rectangle`, a latitude/longitude box whose north and south edges follow their parallels exactly,
  with no great-circle bulge and no hemisphere restriction. Wraps across the antimeridian, and
  `Rectangle::latitude_band` covers bands and polar caps.
- `Ellipse`, an elliptical area given as a centre, angular semi-axes, and the bearing of the major
  axis clockwise from north. `Ellipse::circle` is the circular case.
- `Geodetic` and `LatLon` types, `EcefState::to_geodetic`, and `Predictor::sub_point` — the geodetic
  point directly beneath the satellite.
- `Predictor::ground_track_iter`, sampling sub-satellite points at a fixed cadence.

### Changed

- `DetectError::WindowTooLong` renders its limit as a humantime span (`1h`) rather than chrono's
  ISO-8601 `Display` (`PT3600S`), so the message names the value in the spelling callers pass back
  in. This promotes `humantime` from a dev-dependency to a dependency.

## [0.1.0] - 2026-07-28

Initial release — a higher-level prediction and observation layer over the `sgp4` crate.

- `Predictor` built from a TLE or OMM (`Elements`): propagate state vectors and compute
  azimuth/elevation/range/range-rate observations from a ground station.
- Pass prediction with transit detection (adaptive scan + root-finding refinement).
- Event/window iterators over any time interval: transits, apsides (apogee/perigee), and
  illumination (sunlit/eclipse) windows, plus raw prediction and observation iterators.
- Compile-time-checked coordinate frames (TEME / ECEF / ENU) and angle units
  (`Degrees` / `Radians`); SI units (metres, m/s) throughout.
- Configurable refinement and per-iterator options, TLE-age reporting for stale elements,
  and an optional `generics` feature exposing the underlying detection building blocks.
