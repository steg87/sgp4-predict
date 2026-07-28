# sgp4-predict — Python bindings

[![Test](https://github.com/steg87/sgp4-predict/actions/workflows/test.yml/badge.svg)](https://github.com/steg87/sgp4-predict/actions/workflows/test.yml)
[![PyPI](https://img.shields.io/pypi/v/sgp4-predict)](https://pypi.org/project/sgp4-predict/)
![License: MIT OR Apache-2.0](https://img.shields.io/pypi/l/sgp4-predict)

Python bindings for the [`sgp4-predict`](https://crates.io/crates/sgp4-predict) Rust library:
satellite pass prediction from TLE or OMM data, with type stubs and native `datetime` support.

```sh
pip install sgp4-predict
```

Requires Python 3.10+.

## Quick start

Find the passes over a ground station and sample each one:

```python
from datetime import datetime, timedelta, timezone
from sgp4_predict import GroundObserver, Interval, Predictor, Tle

tle = Tle(
    "SENTINEL-2C",
    "1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990",
    "2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740",
)
predictor = Predictor.from_tle(tle)

glasgow = GroundObserver(latitude_deg=55.86, longitude_deg=-4.25, altitude=40.0)

start = datetime(2025, 12, 22, tzinfo=timezone.utc)
window = Interval(start=start, end=start + timedelta(days=1))

for transit in predictor.transits_iter(glasgow, window, min_elevation_deg=5.0):
    print(f"AoS {transit.start} → LoS {transit.end} ({transit.duration_seconds:.0f}s)")

    # A Transit is itself an interval, so it can be passed straight back in.
    for t, obs in predictor.observation_iter(glasgow, transit, timedelta(seconds=10)):
        print(f"  {t}  az={obs.azimuth_deg:.1f}°  el={obs.elevation_deg:.1f}°")
```

## `Predictor`

Construct from a `Tle`, or from `Elements` parsed from OMM JSON:

```python
from sgp4_predict import Elements

predictor = Predictor.from_tle(tle)                  # raises ValueError if malformed
predictor = Predictor(Elements.from_json(omm_json))  # CCSDS OMM (Celestrak, Space-Track)

predictor.epoch                  # datetime (UTC) — element epoch
predictor.tle_age_seconds(now)   # float — positive means the epoch is in the past
```

SGP4 accuracy degrades with element age; treat LEO TLEs older than 3–7 days with caution. Fresh
TLEs are available from [CelesTrak](https://celestrak.org).

### Point queries

```python
sv    = predictor.propagate(t)               # StateVectorTeme
obs   = predictor.observe_at(t, observer)    # Observation
state = predictor.illumination_state(t)      # IlluminationState.Sunlit or .Eclipse

# The pass in progress at t, or None
transit = predictor.detect_transit(t, observer, min_elevation_deg=5.0)

# Peak elevation within an interval; raises RuntimeError if there is no peak
t_peak, obs_peak = predictor.max_elevation(observer, window)
```

### Iterators

All iterators are lazy. Every method taking a time range accepts any object with `.start` and
`.end` datetime properties — an `Interval`, a `Transit`, an `Illumination`, or your own type.

```python
for t, sv in predictor.prediction_iter(window, step): ...           # StateVectorTeme
for t, obs in predictor.observation_iter(observer, window, step): ...  # Observation
for transit in predictor.transits_iter(observer, window, min_elevation_deg=5.0): ...
for apsis in predictor.apsis_iter(window): ...                      # Apsis
for illumination in predictor.illumination_iter(window): ...        # Illumination
```

`Predictor.with_refinement(Refinement(...))` returns a copy with a different root-finder
configuration for event times; `Refinement` exposes `time_tolerance` (seconds, default `1e-3`) and
`max_iter` (default `100`).

## Return types

```python
transit.start / .end          # datetime (UTC) — AoS / LoS
transit.duration_seconds      # float

obs.azimuth_deg               # float — degrees from north, clockwise, in (-180, 180]
obs.elevation_deg             # float — degrees above the horizon
obs.range                     # float — metres
obs.range_rate                # float — m/s, positive = receding

apsis.time                    # datetime (UTC)
apsis.event                   # ApsisEvent.Apogee or .Perigee
apsis.altitude                # float — metres above the WGS-84 equatorial radius

illumination.start / .end     # datetime (UTC)
illumination.state            # IlluminationState.Sunlit or .Eclipse
illumination.duration_seconds # float
```

`Transit` and `Illumination` are themselves intervals — `isinstance(transit, IntervalRange)` is
`True`, and either can be passed wherever a window is expected.

## Coordinate frames

`propagate()` returns a `StateVectorTeme`. The frame chain is available step by step if you need an
intermediate:

```python
sv_teme = predictor.propagate(t)   # StateVectorTeme
sv_ecef = sv_teme.to_ecef(t)       # StateVectorEcef  (GMST rotation)
sv_enu  = sv_ecef.to_enu(gs)       # StateVectorEnu   (geodetic to local ENU)
obs     = sv_enu.to_observation()  # equivalent to predictor.observe_at(t, gs)
```

All three expose `.position` and `.velocity` as `Vec3(x, y, z)`.

## Units

SI throughout: positions in metres, velocities and range rate in m/s, apsis altitude in metres above
the WGS-84 equatorial radius. Angles are plain floats in degrees — every angular name is suffixed
`_deg`.

Azimuth is measured clockwise from north over `(-180, 180]`, so a southwesterly bearing is negative
rather than the `[0, 360)` most tracking software reports.

## Development

```sh
cd sgp4-predict-py/
uv sync --extra dev   # create .venv and install dev dependencies
make dev              # compile the Rust extension in-place (maturin develop)
make test             # compile + run pytest
make lint             # ruff check --fix + ruff format
```

`make` targets use `uv run`, so no venv activation is needed.

Type stubs live in two files: `python/sgp4_predict/_sgp4_predict/__init__.pyi` is generated by
`make stubs` after Rust API changes and is not committed, while
`python/sgp4_predict/__init__.pyi` is hand-maintained and owns `IntervalRange`, `Interval`, and the
typed `Predictor` overrides.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](https://github.com/steg87/sgp4-predict/blob/main/LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](https://github.com/steg87/sgp4-predict/blob/main/LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
this crate by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.
