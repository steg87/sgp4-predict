# Changelog

All notable changes to `sgp4-predict` (the library crate) are documented here.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Release automation extracts the section whose heading matches the crate version
in the release PR, so keep each version's notes under a `## [x.y.z] - YYYY-MM-DD`
heading. Add work-in-progress notes under `## [Unreleased]`.

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
