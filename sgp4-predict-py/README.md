# sgp4-predict — Python bindings

Python bindings for the [`sgp4-predict`](https://crates.io/crates/sgp4-predict) Rust library, providing typed satellite pass prediction from Two-Line Element (TLE) data.

## Installation

```sh
pip install sgp4-predict
```

Requires Python 3.10+.

## Quick start

```python
from datetime import datetime, timedelta, timezone
from sgp4_predict import Satellite, GroundStation, Predictor

# Sentinel-2C TLE
sat = Satellite(
    "SENTINEL-2C",
    "1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990",
    "2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740",
)

predictor = Predictor(sat)

# Ground station: Glasgow
glasgow = GroundStation(lat_deg=55.86, lon_deg=-4.25, altitude=40.0)

# Find passes over the next 24 hours
start = datetime(2025, 12, 22, tzinfo=timezone.utc)
end = start + timedelta(days=1)

for transit in predictor.transits_iter(glasgow, start, end, min_elevation_deg=5.0):
    print(f"Pass: {transit.start} → {transit.end} ({transit.duration_seconds:.0f}s)")
```

## Core concepts

### TLE data

TLEs are the standard input format for SGP4 propagation. Fresh TLEs can be obtained from sources such as [CelesTrak](https://celestrak.org). SGP4 accuracy degrades with TLE age — for LEO satellites, TLEs older than 3–7 days should be treated with caution. Use `predictor.tle_age_seconds(now)` to check.

### Units

All values are in SI units unless noted otherwise:

| Quantity | Unit |
|---|---|
| Position | metres |
| Velocity | m/s |
| Range | metres |
| Range rate | m/s (positive = receding) |
| Azimuth / elevation | radians (use `_deg` properties for degrees) |
| Altitude (apsis) | metres above WGS-84 equatorial radius |
| `GroundStation` lat/lon input | degrees |
| `GroundStation` altitude input | metres |

## API reference

### `Satellite`

Holds the raw TLE strings. No parsing happens here.

```python
sat = Satellite(id="ISS", line1="1 25544U ...", line2="2 25544 ...")

sat.id      # str
sat.line1   # str
sat.line2   # str
```

### `GroundStation`

A fixed point on Earth's surface. Lat/lon are accepted in degrees.

```python
gs = GroundStation(lat_deg=51.5, lon_deg=-0.1, altitude=10.0)

gs.lat_deg   # float — geodetic latitude (degrees, positive north)
gs.lon_deg   # float — geodetic longitude (degrees, positive east)
gs.altitude  # float — metres above WGS-84 ellipsoid
```

### `Predictor`

The main entry point. Parses the TLE and pre-computes SGP4 constants.

```python
p = Predictor(sat)           # raises ValueError on malformed TLE
p.epoch                      # datetime (UTC) — TLE epoch
p.tle_age_seconds(now)       # float — seconds since TLE epoch (positive = past)
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

All iterators are lazy and implement the Python iterator protocol.

```python
# State vectors at regular intervals
for t, sv in p.prediction_iter(start, end, step):
    ...  # t: datetime, sv: StateVectorTeme

# Observations at regular intervals
for t, obs in p.observation_iter(observer, start, end, step):
    ...  # t: datetime, obs: Observation

# Visible passes
for transit in p.transits_iter(observer, start, end, min_elevation_deg=5.0):
    ...  # Transit

# Apogee / perigee events
for apsis in p.apsis_iter(start, end):
    ...  # Apsis

# Sunlit / eclipse windows
for window in p.illumination_iter(start, end):
    ...  # Illumination
```

#### Transit detection and peak elevation

```python
# Detect whether a transit is in progress at time t
# Returns Transit or None
transit = p.detect_transit(t, observer, min_elevation_deg=5.0)

# Find the peak elevation moment within an interval
# Returns (datetime, Observation) — raises RuntimeError if no peak found
t_peak, obs_peak = p.max_elevation(start, end, observer)
```

#### Illumination state

```python
from sgp4_predict import IlluminationState

state = p.illumination_state(t)  # IlluminationState.Sunlit or .Eclipse
```

### Return types

#### `Transit`

A window during which the satellite is above the minimum elevation.

```python
transit.start             # datetime (UTC) — Acquisition of Signal (AoS)
transit.end               # datetime (UTC) — Loss of Signal (LoS)
transit.duration_seconds  # float
```

#### `Observation`

Point observation from a ground station.

```python
obs.azimuth       # float — radians, 0 = North, clockwise
obs.elevation     # float — radians above horizon
obs.range         # float — metres
obs.range_rate    # float — m/s, positive = receding

obs.azimuth_deg   # float — degrees
obs.elevation_deg # float — degrees
```

#### `Apsis` and `ApsisEvent`

```python
from sgp4_predict import ApsisEvent

apsis.time      # datetime (UTC)
apsis.event     # ApsisEvent.Apogee or ApsisEvent.Perigee
apsis.altitude  # float — metres above WGS-84 equatorial radius
```

#### `Illumination` and `IlluminationState`

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
sv_ecef = sv_teme.to_ecef(t)     # StateVectorEcef  (GMST rotation)
sv_enu  = sv_ecef.to_enu(gs)     # StateVectorEnu   (geodetic to local ENU)
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
source .venv/bin/activate
```

### Commands

```sh
make dev    # compile the Rust extension in-place (maturin develop)
make test   # compile + run pytest
make stubs  # regenerate .pyi stub files
make lint   # ruff check + ruff format --check
```

### Stub files

The `.pyi` stub files provide type information for the compiled `_sgp4_predict.so` extension. They are not committed to the repository but are generated on demand:

```sh
make stubs
```

Stubs are built and bundled automatically during the PyPI release CI run. Generate them locally if you want type checking in your editor while working on the bindings.
