//! Integration test for the generic detection iterators: detect equator
//! crossings — an event kind the crate has no bespoke iterator for — using
//! only the public `EventIter` building blocks.

#![cfg(feature = "generics")]

mod common;

use chrono::Duration;
use sgp4_predict::{Direction, EventIter, FixedStep, Predictor};

#[test]
fn test_equator_crossings() {
    let tle = common::create_tle();
    let p = Predictor::from_tle(&tle).unwrap();

    // Sentinel-2C: 14.30821394 rev/day → orbital period ≈ 100.6 min.
    let period = Duration::seconds((86_400.0 / 14.308_213_94) as i64);

    let start = common::datetime("2025-12-20T12:00:00Z");
    let end = start + Duration::hours(12);

    // In the TEME frame the equator is the plane z = 0, so equator
    // crossings are the zero crossings of the satellite's z coordinate:
    // rising = northward (ascending node), falling = southward.
    let predictor = p.clone();
    let crossings = EventIter::builder()
        .interval(start..end)
        .function(move |t| Ok(predictor.propagate(t)?.position.z))
        .step(FixedStep(Duration::seconds(60)))
        .build()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    // Two crossings per orbit: ~7 orbits in 12 h → 14 or 15 crossings.
    let expected = 2.0 * 12.0 * 3600.0 / period.num_seconds() as f64;
    assert!(
        (crossings.len() as f64 - expected).abs() <= 1.0,
        "expected ≈{expected:.1} crossings in 12 h, found {}",
        crossings.len()
    );

    // Crossings strictly alternate between northward and southward.
    for pair in crossings.windows(2) {
        assert_ne!(
            pair[0].direction, pair[1].direction,
            "consecutive equator crossings must alternate direction"
        );
    }

    // At each refined crossing time the satellite is on the equator plane:
    // |z| below 100 m (the root tolerance is far tighter; z changes at
    // km/s, so 100 m corresponds to ~15 ms of time error).
    for crossing in &crossings {
        let z = p.propagate(crossing.time).unwrap().position.z;
        assert!(
            z.abs() < 100.0,
            "|z| = {:.1} m at refined crossing {}",
            z.abs(),
            crossing.time
        );
    }

    // Consecutive same-direction crossings are one orbital period apart
    // (within a minute: the node precesses slightly between orbits).
    let northward: Vec<_> = crossings
        .iter()
        .filter(|c| c.direction == Direction::Rising)
        .collect();
    assert!(northward.len() >= 6);
    for pair in northward.windows(2) {
        let gap = pair[1].time - pair[0].time;
        assert!(
            (gap - period).num_seconds().abs() < 60,
            "ascending nodes {} and {} are {gap} apart, expected ≈{period}",
            pair[0].time,
            pair[1].time
        );
    }
}
