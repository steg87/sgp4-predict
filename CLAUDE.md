# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
cargo build                    # build the library
cargo check                    # fast type-check without full compile
cargo test --all-targets --all-features  # run all tests
cargo test <name>              # run a single test by name (e.g. cargo test test_brent_cubic)
cargo clippy                   # lint
```

## Architecture

This is a Rust library (`sgp4-predict`) wrapping the `sgp4` crate to provide higher-level prediction and observation iterators for satellite passes.

### Entry point: `Predictor`

`lib.rs` defines `Predictor` as the main struct. It is constructed from any type implementing `Satellite` (a supertrait of `HasId + HasTle`). It exposes:
- `propagate(t)` → `StateVector<Teme>` — raw SGP4 propagation at a moment in time
- `observe_at(t, observer)` → `Observation` — azimuth/elevation/range/range_rate from a ground location
- `prediction_iter(interval, step)` → `PredictionIter`
- `observation_iter(observer, interval, step)` → `ObservationIter`
- `transits_iter(observer, interval, min_elevation)` → `TransitIter`
- `apsis_iter(interval)` → `ApsisIter`

### Type-safe coordinate frames

`frames.rs` uses phantom marker structs (`Teme`, `Ecef`, `Enu`) to make coordinate frame tracking a compile-time guarantee. `StateVector<F>`, `Position<F>`, and `Velocity<F>` in `vectors.rs` are all generic over frame. Conversion methods are implemented directly on the concrete instantiations:

- `StateVector<Teme>::to_ecef(t)` — GMST rotation (Z-axis) to ECEF
- `StateVector<Ecef>::to_enu(observer)` — geodetic to local East-North-Up
- `StateVector<Enu>::to_observation()` / `to_elevation()` — final observables

**All coordinates are in SI units (meters, m/s).** The `sgp4` crate outputs km/km·s⁻¹; conversion happens in `predict.rs` in the `From<sgp4::Prediction>` impl.

**Observer lat/lon must be in radians.**

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
