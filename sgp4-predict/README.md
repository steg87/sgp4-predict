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

`Predictor` also finds apsides (`apsis_iter`), sunlit and eclipse windows (`illumination_iter`) and
area-of-interest overpasses (`aoi_iter`), and answers point queries such as `propagate`,
`observe_at` and `sub_point`. Each detection method has a `_with_opts` sibling taking scan steps and
other tuning knobs, and `Predictor::with_refinement` configures the root finder that pins down event
times. See the [`Predictor` docs](https://docs.rs/sgp4-predict/latest/sgp4_predict/struct.Predictor.html).

## Handling errors

Every iterator yields `Result`, which is the `let transit = transit?;` above.
[`FallibleIter`](https://docs.rs/sgp4-predict/latest/sgp4_predict/trait.FallibleIter.html) moves
that onto the iterator: skip the failures that affect a single event, or stop once they persist.
The `resilient_pass_scan` example in [`tests/examples.rs`](tests/examples.rs) uses both in one scan.

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

Edges are **great-circle arcs**, so two vertices at the same latitude are not joined along the
parallel: the arc bows toward the nearer pole, by about 0.05° for the 7° box above and roughly 8°
for vertices a quarter of the globe apart. Densify edges that long. A polygon must also fit inside a
hemisphere — polar caps, equator-spanning and antimeridian-spanning areas are all fine, a region
larger than half the globe is not.

When the region really is "these latitudes by these longitudes", use `Rectangle` instead — its north
and south edges follow their parallels exactly, it has no hemisphere restriction, and it wraps across
the antimeridian:

```rust,no_run
use sgp4_predict::{Degrees, LatLon, Rectangle};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let scotland = Rectangle::new(
    LatLon { latitude: Degrees(54.0), longitude: Degrees(-8.0) },
    LatLon { latitude: Degrees(60.0), longitude: Degrees(-1.0) },
)?;

// Runs eastward from the south-west corner, so this wraps the antimeridian.
let pacific = Rectangle::new(
    (Degrees(-20.0), Degrees(160.0)),
    (Degrees(20.0), Degrees(-160.0)),
)?;

let arctic = Rectangle::latitude_band(Degrees(66.5), Degrees(90.0))?;
# Ok(())
# }
```

`Ellipse` covers circular and elliptical footprints. Semi-axes are angular, and the bearing turns the
first of them clockwise from north; either may be the longer:

```rust,no_run
use sgp4_predict::{Degrees, Ellipse, LatLon};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
// Roughly 300 km by 120 km, major axis pointing north-east. A degree of arc is
// about 111.2 km on the ground.
let north_sea = Ellipse::new(
    LatLon { latitude: Degrees(56.0), longitude: Degrees(2.0) },
    Degrees(2.7),
    Degrees(1.1),
    Degrees(45.0),
)?;

// A circular area 500 km across.
let cape_town = Ellipse::circle((Degrees(-33.9), Degrees(18.4)), Degrees(2.25))?;
# Ok(())
# }
```

Implement `Area` on your own type for other shapes; the docs for the trait give the contract its
`signed_angular_offset` must satisfy.

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
