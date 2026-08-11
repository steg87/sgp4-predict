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
```

SGP4 accuracy degrades with element age — check `predictor.tle_age_seconds(now)` and treat LEO TLEs
older than 3–7 days with caution. Fresh TLEs are available from [CelesTrak](https://celestrak.org).

Besides `transits_iter`, `Predictor` yields apsides, sunlit and eclipse windows, ground-track
points, area overpasses and raw state vectors, and answers point queries such as `propagate`,
`observe_at` and `sub_point`. The package ships type stubs, so an IDE or `help(Predictor)` lists
them with their signatures and units.

Every method taking a time range accepts any object with `.start` and `.end` datetime properties —
an `Interval`, a `Transit`, an `Illumination`, an `AoiWindow`, or your own type. All iterators are
lazy.

`Predictor.with_refinement(Refinement(...))` returns a copy with a different root-finder
configuration for event times.

## Areas of interest

An area is a region on the ground; `aoi_iter` yields the windows in which it is within the
payload's reach. Points are `LatLon` objects, `Geodetic` objects whose altitude is ignored — so a
`sub_point` result can be passed straight in — or plain `(latitude_deg, longitude_deg)` tuples.

```python
from sgp4_predict import Circle, LatLon, Polygon, Rectangle

# An arbitrary ring. Concave and self-intersecting rings are both fine, the ring
# closes itself, and vertex order does not matter.
scotland = Polygon([(54.0, -8.0), (54.0, -1.0), (60.0, -1.0), LatLon(60.0, -8.0)])

# A latitude/longitude box, whose north and south edges follow their parallels
# exactly. Runs eastward from the south-west corner, so this one wraps the
# antimeridian.
pacific = Rectangle((-20.0, 160.0), (20.0, -160.0))
arctic = Rectangle.latitude_band(66.5, 90.0)

# A circular area 500 km across. The radius is angular — a degree of arc is about
# 111.2 km on the ground.
cape_town = Circle((-33.9, 18.4), radius_deg=2.25)

for overpass in predictor.aoi_iter(scotland, window):
    print(overpass.start, overpass.end)
```

A malformed area raises `ValueError` — fewer than three distinct vertices, a latitude outside
`[-90, 90]`, a `nan` or infinite coordinate, a polygon larger than a hemisphere, an empty box, or
a circle radius outside `(0, 90)`.

By default a window opens when the ground track itself crosses into the area. Pass
`max_off_nadir_deg` — the half-angle of the satellite's field of regard, the largest nadir angle the
payload can be slewed to — and the window instead covers whenever the area is within reach.
`coverage` chooses whether any part of the area or all of it must be in reach:

```python
from sgp4_predict import Coverage

for window in predictor.aoi_iter(cape_town, interval, max_off_nadir_deg=30.0):
    print(window.start, window.end)

# Every part of the area reachable at once, rather than any part of it.
predictor.aoi_iter(cape_town, interval, max_off_nadir_deg=30.0, coverage=Coverage.Full)
```

A window longer than `max_window_duration` — one hour by default — raises `RuntimeError` rather than
being yielded, since a window that long usually means the area is bigger than intended. Raise the
cap for an area that really is near-global:

```python
band = Rectangle.latitude_band(-90.0, 60.0)   # ~85 min of each 100-minute orbit
predictor.aoi_iter(band, window, max_window_duration=timedelta(hours=2))
```

An area the ground track never leaves at all — a whole-Earth box, or a band wider than the orbit's
inclination reaches — has no window end to find, so it raises whatever the cap is set to.

`min_step` is the shortest crossing the scan is guaranteed to see. Lower it below the default second
for an area the ground track crosses faster than that. It also raises the scan's ten-minute upper
bound wherever it exceeds it, so a `min_step` above that pins every step there and a small area is
passed straight over.

Every detection method takes its tuning the same way, as keyword-only arguments left at the
library's defaults unless passed.

`Polygon` edges are **great-circle arcs**, so two vertices at the same latitude are not joined along
the parallel: the arc bows toward the nearer pole, by about 0.05° for the 7° ring above and roughly
8° for vertices a quarter of the globe apart. Densify edges that long, or use `Rectangle` when the
region really is "these latitudes by these longitudes". A polygon must also fit inside a hemisphere —
polar caps, equator-spanning and antimeridian-spanning areas are all fine, a region larger than half
the globe is not.

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
