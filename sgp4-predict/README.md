# sgp4-predict

[![Crates.io](https://img.shields.io/crates/v/sgp4-predict)](https://crates.io/crates/sgp4-predict)
[![docs.rs](https://img.shields.io/docsrs/sgp4-predict)](https://docs.rs/sgp4-predict)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/sgp4-predict)](../LICENSE-MIT)

A Rust library wrapping the [`sgp4`](https://crates.io/crates/sgp4) crate to provide higher-level satellite prediction. Given a TLE, it can propagate state vectors, compute ground observations, iterate over passes, detect apsides, and query illumination.

## Quick start

Add to your `Cargo.toml`:

```toml
[dependencies]
sgp4-predict = "0.1"
```

The built-in [`Tle`] and [`GroundObserver`] types get you up and running immediately:

```rust
use sgp4_predict::{GroundObserver, Predictor, Tle};
use chrono::{Duration, Utc};

// Parse a 3-line element set (name + two TLE lines)
let tle: Tle = "\
    SENTINEL-2C\n\
    1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990\n\
    2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740"
    .parse()?;

// Or construct directly
let tle = Tle::new("SENTINEL-2C", line_1, line_2);

let predictor = Predictor::from_tle(&tle)?;

// Glasgow ground station: lat/lon in degrees, altitude in metres
let glasgow = GroundObserver::new(55.86, -4.25, 40.0);

let start = Utc::now();
let end = start + Duration::days(1);

for transit in predictor.transits_iter(&glasgow, start..end, 5.0) {
    let transit = transit?;
    println!("AoS: {}  LoS: {}", transit.start, transit.end);
}
```

### Sampling observations over a pass

```rust
use chrono::Duration;

// Transit implements IntervalRange, so pass it directly as the interval
for result in predictor.observation_iter(&glasgow, transit, Duration::seconds(10)).include_end() {
    let (t, obs) = result?;
    println!(
        "{} az={:.1}° el={:.1}° range={:.0}km",
        t,
        obs.azimuth_deg(),
        obs.elevation_deg(),
        obs.range / 1000.0,
    );
}
```

### Illumination

```rust
use sgp4_predict::IlluminationState;

match predictor.illumination_state(t)? {
    IlluminationState::Sunlit  => println!("sunlit"),
    IlluminationState::Eclipse => println!("in eclipse"),
}
```

### Detecting a live pass

If you receive a signal mid-pass and need to recover the full window:

```rust
use chrono::Duration;

let transit = predictor
    .detect_transit(now, &glasgow, 5.0, Duration::seconds(30), Duration::hours(1))?
    .expect("satellite is not overhead");
```

### Apsides

```rust
use sgp4_predict::ApsisEvent;

for apsis in predictor.apsis_iter(start..end) {
    let apsis = apsis?;
    match apsis.event {
        ApsisEvent::Apogee  => println!("apogee  at {}", apsis.time),
        ApsisEvent::Perigee => println!("perigee at {}", apsis.time),
    }
}
```

## Custom satellite and observer types

If your application already has types that hold TLE data or ground station coordinates, you can implement the traits directly rather than converting to `Tle` / `GroundObserver`:

```rust
use sgp4_predict::{HasId, HasTle, Observer, Predictor};

// Any type with an id and TLE lines becomes a Satellite automatically
struct MyRecord {
    norad_id: String,
    tle_line1: String,
    tle_line2: String,
}

impl HasId for MyRecord {
    fn id(&self) -> &str { &self.norad_id }
}

impl HasTle for MyRecord {
    fn line_1(&self) -> &str { &self.tle_line1 }
    fn line_2(&self) -> &str { &self.tle_line2 }
}

// Any type with lat/lon/alt becomes an Observer
struct Site {
    lat: f64,
    lon: f64,
    elevation_m: f64,
}

impl Observer for Site {
    fn latitude_deg(&self) -> f64  { self.lat }
    fn longitude_deg(&self) -> f64 { self.lon }
    fn altitude(&self) -> f64      { self.elevation_m }
}

let predictor = Predictor::from_tle(&my_record)?;
```

The `Satellite` supertrait is a blanket impl — any type implementing both `HasId` and `HasTle` satisfies it automatically.

## Units

All quantities are SI. Angles follow a consistent convention: **inputs are in degrees, outputs are in radians**, with `_deg` convenience methods provided.

| Quantity                         | Unit                      |
|----------------------------------|---------------------------|
| Position                         | metres                    |
| Velocity                         | m/s                       |
| Observer lat/lon (input)         | degrees                   |
| Observer altitude                | metres                    |
| `min_elevation_deg` (input)      | degrees                   |
| Azimuth / elevation (output)     | radians                   |
| Range                            | metres                    |
| Range rate                       | m/s (positive = receding) |

Degree equivalents for output angles:

- `Observation::azimuth_deg()`, `Observation::elevation_deg()`

## Examples

See [`tests/examples.rs`](tests/examples.rs) for complete, runnable examples covering common use cases.
