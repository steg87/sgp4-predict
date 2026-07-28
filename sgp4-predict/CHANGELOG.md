# Changelog

All notable changes to `sgp4-predict` (the library crate) are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Record unreleased work under the Unreleased heading below. The release
automation rolls that section into a dated `## [x.y.z] - YYYY-MM-DD` heading and
publishes it verbatim as the GitHub Release body — see `docs/RELEASING.md`.

## [Unreleased]

## [0.1.0] - 2026-07-25

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
