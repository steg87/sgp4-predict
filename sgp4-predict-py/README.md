# sgp4-predict — Python bindings

[![PyPI](https://img.shields.io/pypi/v/sgp4-predict)](https://pypi.org/project/sgp4-predict/)
[![License: MIT OR Apache-2.0](https://img.shields.io/pypi/l/sgp4-predict)](../LICENSE-MIT)

Python bindings for the [`sgp4-predict`](https://crates.io/crates/sgp4-predict) Rust library, providing typed satellite pass prediction from Two-Line Element (TLE) data.

## Installation

```sh
pip install sgp4-predict
```

Requires Python 3.10+.

## Quick start

```python
from datetime import datetime, timedelta, timezone
from sgp4_predict import Tle, Observer, Predictor, Interval

# Sentinel-2C TLE
tle = Tle(
    "SENTINEL-2C",
    "1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990",
    "2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740",
)

predictor = Predictor.from_tle(tle)

# Ground station: Glasgow
glasgow = Observer(lat_deg=55.86, lon_deg=-4.25, altitude=40.0)

# Find passes over the next 24 hours
window = Interval(
    start=datetime(2025, 12, 22, tzinfo=timezone.utc),
    end=datetime(2025, 12, 23, tzinfo=timezone.utc),
)

for transit in predictor.transits_iter(glasgow, window, min_elevation_deg=5.0):
    print(f"Pass: {transit.start} → {transit.end} ({transit.duration_seconds:.0f}s)")

    # Iterate observations over just this transit — pass it directly as an interval
    step = timedelta(seconds=10)
    for t, obs in predictor.observation_iter(glasgow, transit, step):
        print(f"  {t}: az={obs.azimuth_deg:.1f}° el={obs.elevation_deg:.1f}°")
```

## Core concepts

### TLE data

TLEs are the standard input format for SGP4 propagation. Fresh TLEs can be obtained from sources such as [CelesTrak](https://celestrak.org). SGP4 accuracy degrades with TLE age — for LEO satellites, TLEs older than 3–7 days should be treated with caution. Use `predictor.tle_age_seconds(now)` to check.

### Units

All values are in SI units unless noted otherwise:

| Quantity                  | Unit                                        |
| ------------------------- | ------------------------------------------- |
| Position                  | metres                                      |
| Velocity                  | m/s                                         |
| Range                     | metres                                      |
| Range rate                | m/s (positive = receding)                   |
| Azimuth / elevation       | radians (use `_deg` properties for degrees) |
| Altitude (apsis)          | metres above WGS-84 equatorial radius       |
| `Observer` lat/lon input  | degrees                                     |
| `Observer` altitude input | metres                                      |

## API reference

### `Interval` and `IntervalRange`

All iterator methods and `max_elevation` accept any object that exposes `.start` and `.end` `datetime` properties — the `IntervalRange` protocol. `Transit` and `Illumination` satisfy this protocol automatically, so they can be passed directly wherever an interval is expected.

`Interval` is a concrete helper for constructing a plain datetime interval:

```python
from sgp4_predict import Interval, IntervalRange

window = Interval(start=datetime(..., tzinfo=timezone.utc), end=datetime(..., tzinfo=timezone.utc))

# isinstance check works at runtime
assert isinstance(window, IntervalRange)   # True
assert isinstance(transit, IntervalRange)  # True — Transit satisfies the protocol
```

### `Tle`

Holds the raw TLE strings. No parsing happens here.

```python
tle = Tle(satellite_name="ISS", line_1="1 25544U ...", line_2="2 25544 ...")

tle.satellite_name  # str
tle.line_1  # str
tle.line_2  # str
```

### `Observer`

A fixed point on Earth's surface. Lat/lon are accepted in degrees.

```python
gs = Observer(lat_deg=51.5, lon_deg=-0.1, altitude=10.0)

gs.lat_deg   # float — geodetic latitude (degrees, positive north)
gs.lon_deg   # float — geodetic longitude (degrees, positive east)
gs.altitude  # float — metres above WGS-84 ellipsoid
```

### `Predictor`

The main entry point. Pre-computes SGP4 constants.

```python
p = Predictor.from_tle(tle)  # from a Tle object — raises ValueError on malformed TLE
p = Predictor(elements)      # from a pre-parsed Elements (OMM JSON) object
p.epoch                      # datetime (UTC) — element epoch
p.tle_age_seconds(now)       # float — seconds since epoch (positive = past)
```

#### Propagation

```python
sv = p.propagate(t)          # StateVectorTeme — satellite state at UTC datetime t
```

#### Point observation

```python
obs = p.observe_at(t, observer)  # Observation — az/el/range from observer at time t
```

#### Iterators

All iterators are lazy and implement the Python iterator protocol. Every method that takes a time range accepts any `IntervalRange` — an `Interval`, a `Transit`, an `Illumination`, or any object with `.start` and `.end`.

```python
window = Interval(start, end)

# State vectors at regular intervals
for t, sv in p.prediction_iter(window, step):
    ...  # t: datetime, sv: StateVectorTeme

# Observations at regular intervals
for t, obs in p.observation_iter(observer, window, step):
    ...  # t: datetime, obs: Observation

# Visible passes
for transit in p.transits_iter(observer, window, min_elevation_deg=5.0):
    ...  # Transit

# Apogee / perigee events
for apsis in p.apsis_iter(window):
    ...  # Apsis

# Sunlit / eclipse windows
for illumination in p.illumination_iter(window):
    ...  # Illumination
```

Because `Transit` and `Illumination` satisfy `IntervalRange`, you can pass them directly:

```python
# Iterate observations over exactly one transit
for t, obs in p.observation_iter(observer, transit, step):
    ...

# Predict over a sunlit window only
for t, sv in p.prediction_iter(illumination, step):
    ...
```

#### Transit detection and peak elevation

```python
# Detect whether a transit is in progress at time t
# Returns Transit or None
transit = p.detect_transit(t, observer, min_elevation_deg=5.0)

# Find the peak elevation moment within an interval
# Returns (datetime, Observation) — raises RuntimeError if no peak found
t_peak, obs_peak = p.max_elevation(observer, window)
```

#### Illumination state

```python
from sgp4_predict import IlluminationState

state = p.illumination_state(t)  # IlluminationState.Sunlit or .Eclipse
```

### Return types

#### `Transit`

A window during which the satellite is above the minimum elevation. Satisfies `IntervalRange`.

```python
transit.start             # datetime (UTC) — Acquisition of Signal (AoS)
transit.end               # datetime (UTC) — Loss of Signal (LoS)
transit.duration_seconds  # float
```

#### `Observation`

Point observation from a ground station.

```python
obs.azimuth_deg   # float — degrees, 0 = North, clockwise
obs.elevation_deg # float — degrees above horizon
obs.range         # float — metres
obs.range_rate    # float — m/s, positive = receding
```

#### `Apsis` and `ApsisEvent`

```python
from sgp4_predict import ApsisEvent

apsis.time      # datetime (UTC)
apsis.event     # ApsisEvent.Apogee or ApsisEvent.Perigee
apsis.altitude  # float — metres above WGS-84 equatorial radius
```

#### `Illumination` and `IlluminationState`

A contiguous sunlit or eclipse window. Satisfies `IntervalRange`.

```python
from sgp4_predict import IlluminationState

window.start             # datetime (UTC)
window.end               # datetime (UTC)
window.state             # IlluminationState.Sunlit or .Eclipse
window.duration_seconds  # float
```

### Coordinate frames

`propagate()` returns a `StateVectorTeme`. You can walk the full frame chain manually:

```python
sv_teme = p.propagate(t)          # StateVectorTeme
sv_ecef = sv_teme.to_ecef(t)      # StateVectorEcef  (GMST rotation)
sv_enu  = sv_ecef.to_enu(gs)      # StateVectorEnu   (geodetic to local ENU)
obs     = sv_enu.to_observation() # Observation

# Equivalent shorthand:
obs = p.observe_at(t, gs)
```

All three state vector types expose `.position` and `.velocity` as `Vec3(x, y, z)` in metres / m/s.

### Advanced: `Refinement`

Root-finder tolerances for transit boundary and peak-elevation search. Newton-Raphson is tried first; Brent's method is the bracketed fallback.

```python
from sgp4_predict import Refinement

ref = Refinement()
ref.nr_tolerance   = 1e-9   # radians (default)
ref.nr_max_iter    = 50     # (default)
ref.brent_tolerance = 1e-9  # (default)
ref.brent_max_iter  = 100   # (default)

p2 = p.with_refinement(ref)
```

## Local development

### Setup

```sh
cd sgp4-predict-py/
uv sync --extra dev
```

### Commands

```sh
make dev    # compile the Rust extension in-place (maturin develop)
make test   # compile + run pytest
make stubs  # regenerate _sgp4_predict/__init__.pyi stub file
make lint   # ruff check + ruff format --check
```

No venv activation needed — `make` targets use `uv run` and resolve the local `.venv` automatically.

### Stub files

Type stubs live in two files with different ownership:

| File                                             | Ownership                                                                               |
| ------------------------------------------------ | --------------------------------------------------------------------------------------- |
| `python/sgp4_predict/_sgp4_predict/__init__.pyi` | Auto-generated — run `make stubs` to update after Rust changes                          |
| `python/sgp4_predict/__init__.pyi`               | Hand-maintained — owns `IntervalRange`, `Interval`, and the typed `Predictor` overrides |

The hand-maintained stub is committed to the repository. The auto-generated one is also committed so that type checkers work without a build step.
