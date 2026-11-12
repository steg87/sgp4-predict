# Changelog

All notable changes to `sgp4-predict` (the library crate) are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Record unreleased work under the Unreleased heading below. The release
automation rolls that section into a dated `## [x.y.z] - YYYY-MM-DD` heading and
publishes it verbatim as the GitHub Release body — see `docs/RELEASING.md`.

## [Unreleased]

### Added

- `Predictor::osculating_elements` and `TemeState::keplerian` return a `KeplerianElements` set (semi-major axis, eccentricity, angles, period, perigee and apogee altitude).
- `Predictor::mean_elements` returns the published `MeanElements` in SI units, including `bstar` and the mean-motion derivatives.
- `Error::NotBound`, for a state on a parabolic or hyperbolic orbit.

## [0.6.0] - 2026-11-12

### Added

- `TleError` is re-exported, so a `TryFrom<&YourType> for Elements` impl can name its error type.
- `FromStr for Predictor`: parse an element set directly, `text.parse::<Predictor>()`.
- `Tle::from_str` accepts a 2-line element set with no name, naming it `NORAD-<id>` from line 1.

### Changed

- **Breaking:** `Predictor::new` accepts anything convertible to `Elements` (`Elements`, `Tle`, `&Tle`, or your own type via `TryFrom<&YourType> for Elements`). `Predictor::from_tle` and the `TleRecord` trait are removed; replace `Predictor::from_tle(&tle)` with `Predictor::new(&tle)`, and `impl TleRecord for YourType` with `impl TryFrom<&YourType> for Elements`. `Predictor::new` is generic, so an inline argument such as `Predictor::new(serde_json::from_str(s)?)` needs a type annotation: `serde_json::from_str::<Elements>(s)?`.

## [0.5.0] - 2026-11-11

### Added

- `EventIterBuilder::ignore_falling_crossings` and `CrossingDetector::ignore_falling_crossings`: yield only rising crossings.

### Changed

- **Breaking:** `Predictor::arg_lat_iter` yields only the times the satellite reaches the requested argument, as `DateTime<Utc>`; `ArgLatEvent` and `ArgLatCrossing` are removed. For the opposite point, pass the argument plus 180°.

## [0.4.0] - 2026-11-11

### Changed

- **Breaking:** `EventFunction::sample` returns `Result<Option<Sample>>`, and the closures passed to
  `function` and `function_with_rate` return `Result<Option<f64>>` and
  `Result<Option<(f64, f64)>>`. `None` means the function has no value at that time and reads as
  negative, so a window closes at the edge of the function's domain. Wrap existing results in
  `Some`.

## [0.3.2] - 2026-11-11

### Added

- `Predictor::arg_lat_iter`: events where the satellite reaches a given argument of latitude (and the point 180° round the orbit). `Degrees(0.0)` yields the orbit's nodes.
- `Predictor::argument_of_latitude`: the osculating argument of latitude at a given time.

## [0.3.1] - 2026-10-23

### Changed

- `Refinement::solve` returns the negative endpoint of its final bracket, rather than the endpoint
  closer to the root. Every refined event time can move by up to the refinement tolerance (1 ms by
  default).

### Fixed

- Searching exactly a previously returned `Transit` or `AoiWindow` now finds that window once.
  It could come back twice, or not at all. Transit and area-of-interest window boundaries now lie
  just outside the window, by at most the refinement tolerance.

## [0.3.0] - 2026-09-16

### Added

- `IntervalRange` is implemented for `&T`, so an interval type that is not `Copy` can be passed to
  the iterators by reference instead of being cloned.
- `Predictor::point_at` — where a ground target lies as seen from the satellite. Returns a
  `Pointing`: a unit vector in the spacecraft's LVLH frame, plus slant range and range rate, which
  is what an RF link budget or an instrument-pointing calculation needs.
- `LvlhState` and the `Lvlh` frame marker, with `TemeState::to_lvlh` and `LvlhState::to_pointing`.
  The axes are `Z` along nadir, `Y` along the negative orbit normal, `X` completing the triad
  along-track. `Pointing::off_nadir` derives the nadir angle, in the same geocentric convention as
  `AoiIterOpts::max_off_nadir`.
- `EcefState::to_teme`, the inverse of `EcefState`'s forward transform, frame-drag term included.
- `UnitVector<F>`, a dimensionless vector kind distinct from `Position<F>`.

### Changed

- **Breaking:** the `Observer` trait and the `GroundObserver` struct are removed. Every method
  taking a location now takes `impl Into<GeodeticPoint>`, and `Geodetic` is renamed to
  `GeodeticPoint` — one type for a point on the Earth instead of three spellings of it.

  Replace `GroundObserver::new(Degrees(lat), Degrees(lon), alt)` with a `GeodeticPoint { latitude,
  longitude, altitude }` literal, and an `impl Observer for YourType` with
  `impl From<&YourType> for GeodeticPoint`. Call sites passing `&observer` keep working.

  `ObservationIter` and `TransitIter` lose their lifetime and type parameters as a result:
  `TransitIter<'a, O>` is now `TransitIter`.
- The observer's coordinates are resolved once, when an iterator is constructed, rather than at
  every sample. This matches `Observer`'s documented contract of a fixed point on Earth's surface,
  but is observable to anyone who implemented it with interior mutability.

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
