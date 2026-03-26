# sgp4-predict

[![Crates.io](https://img.shields.io/crates/v/sgp4-predict)](https://crates.io/crates/sgp4-predict)
[![docs.rs](https://img.shields.io/docsrs/sgp4-predict)](https://docs.rs/sgp4-predict)
[![License: MIT OR Apache-2.0](https://img.shields.io/crates/l/sgp4-predict)](../LICENSE-MIT)

A Rust library wrapping the [`sgp4`](https://crates.io/crates/sgp4) crate to provide higher-level satellite prediction. Given a TLE, it can propagate state vectors, compute ground observations, iterate over passes, detect apsides, and query illumination.

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
sgp4-predict = "0.1"
```

### Implementing `Satellite`

`Predictor` accepts any type that implements `HasId` and `HasTle`:

```rust
use sgp4_predict::{HasId, HasTle, Predictor};

struct MyTle {
    name: String,
    line1: String,
    line2: String,
}

impl HasId for MyTle {
    fn id(&self) -> &str { &self.name }
}

impl HasTle for MyTle {
    fn line_1(&self) -> &str { &self.line1 }
    fn line_2(&self) -> &str { &self.line2 }
}

let predictor = Predictor::new(&my_tle)?;
```

### Implementing `Observer`

Pass any type that implements `Observer` as the ground location:

```rust
use sgp4_predict::Observer;

struct GroundStation {
    lat_deg: f64,
    lon_deg: f64,
    alt_m: f64,
}

impl Observer for GroundStation {
    fn latitude_deg(&self) -> f64  { self.lat_deg }
    fn longitude_deg(&self) -> f64 { self.lon_deg }
    fn altitude(&self) -> f64      { self.alt_m }
}

let glasgow = GroundStation { lat_deg: 55.86, lon_deg: -4.25, alt_m: 40.0 };
```

### Predicting passes

```rust
use chrono::{Duration, Utc};
use sgp4_predict::Predictor;

let start = Utc::now();
let end = start + Duration::days(1);

// Observer: lat/lon in degrees, altitude in metres
for transit in predictor.transits_iter(&observer, start..end, 5.0) {
    let transit = transit?;
    println!("AoS: {}  LoS: {}", transit.start, transit.end);
}
```

### Sampling observations over a pass

```rust
use chrono::Duration;

// Transit implements IntervalRange, so pass it directly as the interval
for result in predictor.observation_iter(&observer, transit, Duration::seconds(10)).include_end() {
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
let transit = predictor
    .detect_transit(now, &observer, 5.0)?
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
