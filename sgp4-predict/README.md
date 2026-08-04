# sgp4-predict

[![Test](https://github.com/steg87/sgp4-predict/actions/workflows/test.yml/badge.svg)](https://github.com/steg87/sgp4-predict/actions/workflows/test.yml)
[![Crates.io](https://img.shields.io/crates/v/sgp4-predict)](https://crates.io/crates/sgp4-predict)
[![docs.rs](https://img.shields.io/docsrs/sgp4-predict)](https://docs.rs/sgp4-predict)
[![MSRV](https://img.shields.io/crates/msrv/sgp4-predict)](https://github.com/steg87/sgp4-predict/blob/main/sgp4-predict/Cargo.toml)
![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/sgp4-predict)

Higher-level satellite prediction on top of the [`sgp4`](https://crates.io/crates/sgp4) crate.
Give it a TLE and it will propagate state vectors, compute ground observations, and find passes,
apsides, and sunlit windows over any time range.

```toml
[dependencies]
sgp4-predict = "0.1"
```

## Quick start

Find the passes over a ground station in the next 24 hours, and sample each one:

```rust,no_run
use chrono::{Duration, Utc};
use sgp4_predict::{Degrees, GroundObserver, Predictor, Tle};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tle: Tle = "\
        SENTINEL-2C
        1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990
        2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740"
        .parse()?;

    let predictor = Predictor::from_tle(&tle)?;

    // Glasgow: latitude, longitude, altitude in metres.
    let glasgow = GroundObserver::new(Degrees(55.86), Degrees(-4.25), 40.0);

    let start = Utc::now();
    let interval = start..start + Duration::days(1);

    for transit in predictor.transits_iter(&glasgow, interval, Degrees(5.0)) {
        let transit = transit?;
        println!("AoS {}  LoS {}", transit.start, transit.end);

        // A Transit is itself a time interval, so it can be passed straight back in.
        for observation in predictor.observation_iter(&glasgow, transit, Duration::seconds(10)) {
            let (t, obs) = observation?;
            println!(
                "  {t}  az {:6.1}°  el {:5.1}°  range {:.0} km",
                obs.azimuth.degrees(),
                obs.elevation.degrees(),
                obs.range / 1000.0,
            );
        }
    }
    Ok(())
}
```

## What else `Predictor` can do

| Method | Yields |
|---|---|
| `propagate(t)` | TEME state vector at an instant |
| `observe_at(t, observer)` | azimuth / elevation / range / range rate |
| `sub_point(t)` | the geodetic point directly beneath the satellite |
| `prediction_iter(interval, step)` | state vectors at a fixed cadence |
| `observation_iter(observer, interval, step)` | observations at a fixed cadence |
| `ground_track_iter(interval, step)` | sub-satellite points at a fixed cadence |
| `transits_iter(observer, interval, min_elevation)` | passes above a minimum elevation |
| `detect_transit(t, observer, min_elevation)` | the pass in progress at `t`, if any |
| `max_elevation(interval, observer)` | the peak-elevation moment of a pass |
| `aoi_iter(area, interval)` | windows with the ground track inside an area |
| `detect_aoi(t, area)` | the area window in progress at `t`, if any |
| `apsis_iter(interval)` | apogee and perigee events |
| `illumination_iter(interval)` | sunlit and eclipse windows |
| `illumination_state(t)` | sunlit or in eclipse at an instant |
| `tle_age(now)` | how stale the elements are |

Each detection method has a `_with_opts` sibling taking scan steps and other tuning knobs, and
`Predictor::with_refinement` configures the root finder that pins down event times.

## Areas of interest

`Polygon` describes a region on the ground as a ring of latitude/longitude vertices — concave and
self-intersecting rings are both fine, and the ring closes itself:

```rust,no_run
use chrono::{Duration, Utc};
use sgp4_predict::{Degrees, LatLon, Polygon, Predictor, Tle};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tle: Tle = "\
        SENTINEL-2C
        1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990
        2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740"
        .parse()?;
    let predictor = Predictor::from_tle(&tle)?;

    let area = Polygon::new([
        LatLon { latitude: Degrees(54.0), longitude: Degrees(-8.0) },
        LatLon { latitude: Degrees(54.0), longitude: Degrees(-1.0) },
        LatLon { latitude: Degrees(60.0), longitude: Degrees(-1.0) },
        LatLon { latitude: Degrees(60.0), longitude: Degrees(-8.0) },
    ])?;

    let start = Utc::now();
    for window in predictor.aoi_iter(&area, start..start + Duration::days(7)) {
        let window = window?;
        println!("overhead from {} to {}", window.start, window.end);
    }
    Ok(())
}
```

Edges are **great-circle arcs**, so vertices at the same latitude are not joined along the parallel —
the arc bows toward the nearer pole, growing with the square of the edge's longitude span. The 7° box
above bulges about 0.05° (5 km) north of 60°N; vertices a quarter of the globe apart would reach
roughly 68°N, so densify edges that long. An area must also fit inside a hemisphere;
this permits polar caps, equator-spanning and antimeridian-spanning areas, but not a region larger
than half the globe. Implement `Area` on your own type for other shapes.

## Bring your own types

If your application already has a satellite record or a ground-station type, implement
[`TleRecord`](https://docs.rs/sgp4-predict/latest/sgp4_predict/trait.TleRecord.html) or
[`Observer`](https://docs.rs/sgp4-predict/latest/sgp4_predict/trait.Observer.html) on it and pass
it in directly — every method is generic over these traits:

```rust
use sgp4_predict::{Degrees, Observer};

struct Site {
    lat: f64,
    lon: f64,
    elevation_m: f64,
}

impl Observer for Site {
    fn latitude(&self) -> Degrees  { Degrees(self.lat) }
    fn longitude(&self) -> Degrees { Degrees(self.lon) }
    fn altitude(&self) -> f64      { self.elevation_m }
}
```

## Units

Positions are in **metres** and velocities in **m/s** throughout.

Angles are typed: `Degrees` and `Radians` are distinct, so they cannot be mixed up by accident.
Observer coordinates are `Degrees`; `Observation::azimuth` and `elevation` are `Radians`. Convert
with `.to_degrees()` / `.to_radians()`, or use `.degrees()` / `.radians()` to get a bare `f64`.
Anywhere a `min_elevation` is taken, either unit is accepted.

Azimuth is measured clockwise from north over `(-π, π]`, so a southwesterly bearing is negative.
Call `.normalized()` for the `[0, 2π)` convention that most tracking software reports.

## Orbit Mean-Elements Messages

`sgp4::Elements` (re-exported) deserialises straight from CCSDS OMM JSON as served by Celestrak
and Space-Track, and `Predictor::new` takes it in place of a TLE.

## More

- [API documentation](https://docs.rs/sgp4-predict)
- [`tests/examples.rs`](tests/examples.rs) — runnable end-to-end examples
- Notes on [architecture](https://github.com/steg87/sgp4-predict/blob/main/docs/architecture.md),
  [coordinate frames](https://github.com/steg87/sgp4-predict/blob/main/docs/coordinate-frames.md),
  and [event detection](https://github.com/steg87/sgp4-predict/blob/main/docs/event-detection.md)

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
