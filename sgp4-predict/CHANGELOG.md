# Changelog

All notable changes to `sgp4-predict` (the library crate) are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Record unreleased work under the Unreleased heading below. The release
automation rolls that section into a dated `## [x.y.z] - YYYY-MM-DD` heading and
publishes it verbatim as the GitHub Release body — see `docs/RELEASING.md`.

## [Unreleased]

## [0.2.1] - 2026-08-26

### Added

- `IntervalRange::mid_point`, the instant halfway between an interval's start and end.

## [0.2.0] - 2026-08-24

### Added

- Area-of-interest detection: `Predictor::aoi_iter` and `detect_aoi` yield the `AoiWindow`s during
  which an `Area` is within the payload's reach. `AoiIterOpts::max_off_nadir` is the half-angle of
  the satellite's field of regard — the largest nadir angle the payload can be slewed to — and
  defaults to zero, which detects the ground track itself crossing into the area.
  `AoiIterOpts::coverage` chooses whether any part of the area (`Coverage::Any`, the default) or all
  of it (`Coverage::Full`) must be in reach. `Polygon` describes an arbitrary ring of
  latitude/longitude vertices — concave and self-intersecting rings are supported, with
  `FillRule::NonZero` (default) or `FillRule::EvenOdd` deciding the interior of the latter. The ring
  closes implicitly and vertex order does not matter. Implement `Area` for other shapes; it has two
  methods, `signed_angular_offset` and `max_angular_distance`, the latter with a supplied
  implementation covering any area whose offset is exact.
- `Rectangle`, a latitude/longitude box whose north and south edges follow their parallels exactly,
  with no great-circle bulge and no hemisphere restriction. Wraps across the antimeridian, and
  `Rectangle::latitude_band` covers bands and polar caps.
- `Circle`, a spherical cap given as a centre and an angular radius.
- `Geodetic` and `LatLon` types, `EcefState::to_geodetic`, and `Predictor::sub_point` — the geodetic
  point directly beneath the satellite.
- `Predictor::ground_track_iter`, sampling sub-satellite points at a fixed cadence.
- The common derives across the public API: every type implements `Debug`, and `Clone`, `Copy`,
  `Default`, `PartialEq`, `Eq`, `PartialOrd`, `Ord` and `Hash` are derived wherever the fields
  support them. `Transit`, `AoiWindow`, `Illumination` and `Window` are `Ord`, so they sort and
  serve as `BTreeMap`/`BTreeSet` keys; types holding an `f64` are `PartialOrd` at most. Every
  error enum is `Clone + PartialEq`, so a whole `Result` can be compared without matching the
  variant out.
- `FallibleIter`, an extension trait on every iterator yielding `Result`, replacing the loop-body
  `?` with one call on the iterator itself. `on_error`, `skip_errors` and `log_errors` consume the
  error and carry on, for the failures that affect a single event; `tolerate_errors(n)` and
  `until_error` stop once errors persist and retain the one that stopped them, for the failures
  that will repeat at every sample. It is in the prelude.
- `TimeWindow`, a trait over the concrete window types (`Transit`, `AoiWindow`, `Illumination`
  and `Window`). Implementing `with_bounds` supplies `clamp_to`, so
  `Illumination` and `Window` gain it and it is written once rather than per type.
- `RootsError`, the payload of `Error::Roots`, is exported alongside `AoiError` and `DetectError`.
  It was reachable through that variant but could not be named.

### Changed

- The overview of the `generics` feature — what the layers are, and a worked equator-crossing
  example — is published in the crate documentation. It lived on a private module, so it appeared
  nowhere in the rendered docs.
- Every public item now carries documentation, enforced by `#![warn(missing_docs)]`.

- Errors that are returned to the caller are no longer also logged. Refinement failures, the
  window-boundary give-up and illumination window failures each emitted a `warn` on the way out,
  which double-reported every error once the caller handled it — and twice over when that handler
  was `log_errors`. Logging is the caller's decision; `FallibleIter` is how to make it.
- The public `Error` enums (`sgp4_predict::Error`, `aoi::Error`, `roots::Error`, `DetectError`) are
  `#[non_exhaustive]`, so adding a variant is no longer a breaking change. A downstream `match` on
  one now needs a `_` arm. The `*Opts` structs stay exhaustive, so `..Default::default()` keeps
  working.
- Every iterator and builder type is `#[must_use]`. Dropping the result of a `*_iter` call, or of a
  builder chain that never reaches `.build()`, now warns instead of silently doing nothing. Pure
  getters and conversions (`Degrees::to_f64`, `Circle::radius`, `Predictor::epoch`, ...) are
  `#[must_use]` too.
- `Transit::clamp` and `AoiWindow::clamp` became `TimeWindow::clamp_to`; callers now need
  `use sgp4_predict::TimeWindow` (it is in the prelude). Behaviour is unchanged. The name avoids
  `Ord::clamp`, which takes precedence in method resolution and would shadow it.
- `DetectError::WindowTooLong` renders its limit as a humantime span (`1h`) rather than chrono's
  ISO-8601 `Display` (`PT3600S`), so the message names the value in the spelling callers pass back
  in. This promotes `humantime` from a dev-dependency to a dependency.

### Fixed

- A `TransitIterOpts` whose `min_step` exceeds its `max_step` no longer steps over passes and
  reports nothing. `ThresholdStep` now raises `max` to `min`, matching how `AoiIterOpts` resolves
  the same pair.

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
