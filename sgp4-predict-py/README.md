# sgp4-predict — Python bindings

[![Test](https://github.com/steg87/sgp4-predict/actions/workflows/test.yml/badge.svg)](https://github.com/steg87/sgp4-predict/actions/workflows/test.yml)
[![PyPI](https://img.shields.io/pypi/v/sgp4-predict)](https://pypi.org/project/sgp4-predict/)
[![Python versions](https://img.shields.io/pypi/pyversions/sgp4-predict)](https://pypi.org/project/sgp4-predict/)
![License: MIT OR Apache-2.0](https://img.shields.io/pypi/l/sgp4-predict)

Python bindings for the [`sgp4-predict`](https://crates.io/crates/sgp4-predict) Rust library:
satellite pass prediction from TLE or OMM data, with type stubs and native `datetime` support.

```sh
pip install sgp4-predict
```

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
point = predictor.sub_point(t)               # Geodetic — the point beneath the satellite
state = predictor.illumination_state(t)      # IlluminationState.Sunlit or .Eclipse

# The pass in progress at t, or None
transit = predictor.detect_transit(t, observer, min_elevation_deg=5.0)

# The area window in progress at t, or None
overpass = predictor.detect_aoi(t, area)

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
for t, point in predictor.ground_track_iter(window, step): ...      # Geodetic
for overpass in predictor.aoi_iter(area, window): ...               # AoiWindow
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

overpass.start / .end         # datetime (UTC) — entry / exit
overpass.duration_seconds     # float

point.latitude_deg            # float — positive north
point.longitude_deg           # float — positive east
point.altitude                # float — metres above the WGS-84 ellipsoid
```

`Transit`, `Illumination` and `AoiWindow` are themselves intervals —
`isinstance(transit, IntervalRange)` is `True`, and any of them can be passed wherever a window is
expected.

## Areas of interest

An area is a region on the ground; `aoi_iter` yields the windows in which the sub-satellite point
lies inside it. Points are `LatLon` objects or plain `(latitude_deg, longitude_deg)` tuples.

```python
from sgp4_predict import Ellipse, LatLon, Polygon, Rectangle

# An arbitrary ring. Concave and self-intersecting rings are both fine, the ring
# closes itself, and vertex order does not matter.
scotland = Polygon([(54.0, -8.0), (54.0, -1.0), (60.0, -1.0), LatLon(60.0, -8.0)])

# A latitude/longitude box, whose north and south edges follow their parallels
# exactly. Runs eastward from the south-west corner, so this one wraps the
# antimeridian.
pacific = Rectangle((-20.0, 160.0), (20.0, -160.0))
arctic = Rectangle.latitude_band(66.5, 90.0)

# An ellipse, roughly 300 km by 120 km, major axis pointing north-east. Semi-axes
# are angular — a degree of arc is about 111.2 km on the ground.
north_sea = Ellipse((56.0, 2.0), semi_major_deg=2.7, semi_minor_deg=1.1, bearing_deg=45.0)
cape_town = Ellipse.circle((-33.9, 18.4), radius_deg=2.25)

for overpass in predictor.aoi_iter(scotland, window):
    print(overpass.start, overpass.end)
```

A malformed area raises `ValueError` — fewer than three distinct vertices, a latitude outside
`[-90, 90]`, a `nan` or infinite coordinate, a polygon larger than a hemisphere, an empty box, or
ellipse semi-axes outside `0 < semi_minor_deg <= semi_major_deg < 90`.

A window longer than `max_window_duration` — one hour by default — raises `RuntimeError` instead of
being yielded, on the assumption that a window that long means the area is bigger than intended. A
near-global area really is that big: a LEO satellite is inside `Rectangle.latitude_band(-90.0, 60.0)`
for about 85 minutes of each 100-minute orbit, so raise the cap for one. The other knob is
`min_step`, the lower bound of the adaptive coarse scan and so the shortest crossing the scan is
guaranteed to see; lower it below the default second for an area the ground track crosses faster
than that.

```python
band = Rectangle.latitude_band(-90.0, 60.0)
predictor.aoi_iter(band, window, max_window_duration=timedelta(hours=2))
predictor.detect_aoi(t, band, max_window_duration=timedelta(hours=2))
```

An area the ground track never leaves at all — a whole-Earth box, or a band wider than the orbit's
inclination reaches — has no window end to find, so it raises whatever the cap is set to.

`Polygon` edges are **great-circle arcs**, so vertices at the same latitude are not joined along the
parallel — the arc bows toward the nearer pole, growing with the square of the edge's longitude span.
The 7° box above bulges about 0.05° (5 km) north of 60°N; vertices a quarter of the globe apart would
reach roughly 68°N, so densify edges that long. Since the bow is always toward the *nearer* pole,
both horizontal edges of a box shift the same way: the region ends up displaced poleward, not merely
enlarged — use `Rectangle` when the region really is "these latitudes by these longitudes". A polygon
must also fit inside a hemisphere; this permits polar caps, equator-spanning and antimeridian-spanning
areas, but not a region larger than half the globe.

Each shape also exposes `signed_angular_offset_deg(point)`, positive inside the area and negative
outside, which is the quantity detection is built on.

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
