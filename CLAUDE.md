# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                    # build the library
cargo check                    # fast type-check without full compile
cargo test --all-targets --all-features  # run all tests
cargo test <name>              # run a single test by name (e.g. cargo test test_brent_cubic)
cargo clippy                   # lint
make lint                      # cargo fmt + clippy (preferred — matches CI and pre-commit hook)
make test                      # full test suite (preferred — matches CI and pre-push hook)
make coverage                  # llvm-cov summary
make validation                # cross-validate against pypredict/skyfield reference data
make benchmark                 # Rust vs pypredict monte carlo benchmark
make docs                      # build cargo docs and open in a browser
```

**Always run `make lint` and `make test` after making changes** to catch formatting, lint, and correctness issues before pushing. CI enforces both.

### Python bindings (`sgp4-predict-py/`)

Run these from within `sgp4-predict-py/`:

```bash
make dev    # compile the Rust extension in-place (maturin develop)
make test   # compile + run pytest
make lint   # ruff check --fix + ruff format (fixes in place, like the Rust make lint)
```

To regenerate stubs after Rust API changes (run from repo root):

```bash
PYO3_PYTHON=sgp4-predict-py/.venv/bin/python \
  cargo run --manifest-path sgp4-predict-py/Cargo.toml --bin stub_gen
```

Note: `make stubs` inside `sgp4-predict-py/` fails when `VIRTUAL_ENV` points elsewhere — use the explicit command above instead.

**Known stub-gen limitation**: pyo3-stub-gen silently drops static methods whose parameters are `&Bound<'_, PyAny>` (e.g. `Elements.from_dict`). Such methods work at runtime but will not appear in `_sgp4_predict/__init__.pyi`. If this becomes a problem, the method signature can be overridden in the hand-maintained `sgp4_predict/__init__.pyi`.

## Architecture

This is a Rust library (`sgp4-predict`) wrapping the `sgp4` crate to provide higher-level prediction and observation iterators for satellite passes. The workspace has two crates: `sgp4-predict/` (the Rust library) and `sgp4-predict-py/` (the Python bindings).

### Entry point: `Predictor`

`sgp4-predict/src/lib.rs` defines `Predictor` as the main struct. It is constructed from any type implementing `TleRecord` (via `Predictor::from_tle`) or from `Elements` (OMM, via `Predictor::new`). It exposes:
- `propagate(t)` → `TemeState` — raw SGP4 propagation at a moment in time
- `observe_at(t, observer)` → `Observation` — azimuth/elevation/range/range_rate from a ground location
- `prediction_iter(interval, step)` → `PredictionIter`
- `observation_iter(observer, interval, step)` → `ObservationIter`
- `transits_iter(observer, interval, min_elevation)` → `TransitIter`
- `apsis_iter(interval)` → `ApsisIter`

### Generic detection (`detect.rs`, opt-in `generics` feature)

The generic event/window iterators in `detect.rs` (`EventIter`, `WindowIter`, `Detector`, `StepStrategy`, ...) power `ApsisIter`, `TransitIter`, and `IlluminationIter` internally, so the module always compiles — but its public re-exports at the crate root are gated behind the off-by-default `generics` Cargo feature to keep the everyday API surface small. `DetectError` stays exported unconditionally because `TransitIter` can surface it (`Error::Detect(WindowTooLong)`). `tests/detect.rs` is gated with `#![cfg(feature = "generics")]`; `make test` and `make lint` use `--all-features` so the gated code stays covered.

### Type-safe coordinate frames

`frames.rs` uses phantom marker structs (`Teme`, `Ecef`, `Enu`) to make coordinate frame tracking a compile-time guarantee. `StateVector<F>`, `Position<F>`, and `Velocity<F>` in `vectors.rs` are all generic over frame. Conversion methods are implemented directly on the concrete instantiations:

- `StateVector<Teme>::to_ecef(t)` — GMST rotation (Z-axis) to ECEF
- `StateVector<Ecef>::to_enu(observer)` — geodetic to local East-North-Up
- `StateVector<Enu>::to_observation()` / `to_elevation()` — final observables

**All coordinates are in SI units (meters, m/s).** The `sgp4` crate outputs km/km·s⁻¹; conversion happens in `sgp4-predict/src/predict.rs` in the `From<sgp4::Prediction>` impl.

**Observer lat/lon are in degrees** (both `GroundObserver` and the `Observer` trait return degrees from `latitude_deg()` / `longitude_deg()`; internal conversions to radians happen inside frame math).

**Python vs Rust naming**: in Rust, `Observer` is the *trait*; the concrete type is `GroundObserver`. In the Python bindings, the class is also named `GroundObserver`.

### Apsis detection (`apsides.rs`)

`ApsisIter` detects apogee and perigee events in the TEME frame with a fixed 60-second step. It monitors the sign of the radial velocity `r · v` (dot product of position and velocity vectors). A sign change brackets an event:
- `r·v > 0 → < 0`: apogee (`ApsisEvent::Apogee`)
- `r·v < 0 → > 0`: perigee (`ApsisEvent::Perigee`)

Brent's method refines the crossing time (no derivative needed; bracket is already known).

### Transit detection (`transits.rs`)

`TransitIter` uses an adaptive step-size strategy: large steps when the satellite is descending or far from `min_elevation`, smaller steps when approaching. On detecting an Outside→Inside transition, it refines the exact crossing time using root finding (`roots.rs`):
1. Newton-Raphson (uses elevation rate as derivative, fast convergence)
2. Falls back to Brent's method (bracketed, guaranteed convergence) if Newton-Raphson fails

### `IntervalRange` trait (`time.rs`)

Both `Range<DateTime<Utc>>` and `Transit` implement `IntervalRange`, so a `Transit` can be passed directly as an interval to `prediction_iter` or `observation_iter` to iterate over a specific pass.

## Conventions

- **Code comments**: keep terse. State the non-obvious fact, not the reasoning behind it or alternatives considered.

## Repo infrastructure

- **Git hooks**: managed by `prek` (`prek.toml`). Pre-commit runs fmt+clippy; pre-push runs test+coverage. Contributors install with `prek install`.
- **CI** (`.github/workflows/`):
  - `test.yml` — runs `cargo test`, `cargo fmt --check`, `cargo clippy`, and `cargo doc` (denying rustdoc warnings). Installs `uv` in the test and docs jobs.
  - `audit.yml` — weekly `cargo audit` for security advisories.
  - `labeler.yml` — auto-labels PRs based on changed files (config in `.github/labeler.yml`).
- **Dependencies**: `serde_yaml` (not `serde_yml`) is used for YAML parsing in dev/tests.

## Domain knowledge

This library operates in the LEO (Low Earth Orbit) domain. Meaningful review of functionality requires expertise in:

- **SGP4 propagation**: the underlying orbital mechanics model, its assumptions, and known limitations (e.g. accuracy degrades beyond ~7 days from TLE epoch).
- **Coordinate frames**: TEME (True Equator Mean Equinox), ECEF (Earth-Centred Earth-Fixed), ENU (East-North-Up). Mistakes in frame conversions produce silently wrong results.
- **Ground station geometry**: azimuth/elevation calculations, horizon masking, atmospheric refraction (not modelled here).
- **Apsis timing**: apogee/perigee detection via radial velocity sign change is correct for near-circular LEO orbits; behaviour near highly elliptical orbits should be verified carefully.
- **Illumination model**: a cylindrical shadow model is used — this is an approximation. It is adequate for most LEO scheduling use cases but will have error near the penumbra boundary.
- **TLE age**: SGP4 accuracy is sensitive to TLE age. `Predictor::tle_age()` exposes this; callers should warn or reject stale TLEs (typically > 3–7 days for LEO).
