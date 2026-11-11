mod common;

use chrono::Duration;
use sgp4_predict::{ArgLatEvent, Degrees, Elements, Error, Predictor, TleRecord};

fn predictor() -> Predictor {
    Predictor::from_tle(common::create_tle()).unwrap()
}

fn run(argument: f64) -> (Predictor, Vec<sgp4_predict::ArgLatCrossing>) {
    let p = predictor();
    let start = common::datetime("2025-12-20T12:00:00Z");
    let events = p
        .arg_lat_iter(start..start + Duration::minutes(300), Degrees(argument))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    (p, events)
}

#[test]
fn test_nodes_alternate_and_lie_on_the_equator() {
    let (p, events) = run(0.0);
    assert!(events.len() >= 5, "expected ~6 nodes in 300 min");
    for pair in events.windows(2) {
        assert_ne!(pair[0].event, pair[1].event);
    }
    for e in &events {
        let s = p.propagate(e.time).unwrap();
        assert!(s.position.z.abs() < 50.0, "z = {} m", s.position.z);
        assert_eq!(e.event == ArgLatEvent::Argument, s.velocity.z > 0.0);
    }
}

// Mean elements of `common::create_tle()`: inclination, argument of perigee
// and mean anomaly (degrees), and mean motion (rev/day). With e ≈ 1e-4 the
// mean argument of latitude at epoch is ω + M, advancing at the mean motion.
// Osculating and mean values differ by a few tenths of a degree, so these
// checks are independent of the vector algebra but only good to that scale.
const INCLINATION: f64 = 98.5671;
const U_EPOCH: f64 = (95.1447 + 264.9872) % 360.0;
const DEG_PER_MIN: f64 = 14.308_213_94 * 360.0 / 1440.0;

#[test]
fn test_event_time_matches_mean_elements() {
    let p = predictor();
    let u0 = 100.0;
    let first = p
        .arg_lat_iter(p.epoch()..p.epoch() + Duration::minutes(60), Degrees(u0))
        .next()
        .unwrap()
        .unwrap();
    assert_eq!(first.event, ArgLatEvent::Argument);
    let expected = (u0 - U_EPOCH) / DEG_PER_MIN * 60.0;
    let got = (first.time - p.epoch()).num_milliseconds() as f64 / 1e3;
    assert!((got - expected).abs() < 30.0, "{got} s vs {expected} s");
}

#[test]
fn test_latitude_at_events_follows_inclination() {
    // sin(latitude) = sin(i)·sin(u): +φ at u₀, −φ at u₀ + 180°.
    let (p, events) = run(30.0);
    assert!(!events.is_empty());
    let want = (INCLINATION.to_radians().sin() * 30f64.to_radians().sin()).asin();
    for e in &events {
        let s = p.propagate(e.time).unwrap();
        let r = (s.position.x.powi(2) + s.position.y.powi(2) + s.position.z.powi(2)).sqrt();
        let lat = (s.position.z / r).asin().to_degrees();
        let want = match e.event {
            ArgLatEvent::Argument => want.to_degrees(),
            ArgLatEvent::Antipode => -want.to_degrees(),
        };
        assert!((lat - want).abs() < 0.5, "{:?}: {lat}° vs {want}°", e.event);
    }
}

#[test]
fn test_argument_of_latitude_at_epoch() {
    let p = predictor();
    let u = p.argument_of_latitude(p.epoch()).unwrap().degrees();
    let diff = (u - U_EPOCH + 180.0).rem_euclid(360.0) - 180.0;
    assert!(diff.abs() < 0.5, "u = {u}°");
}

#[test]
fn test_non_finite_argument_errors_once_and_ends() {
    let p = predictor();
    let start = common::datetime("2025-12-20T12:00:00Z");
    let mut it = p.arg_lat_iter(start..start + Duration::minutes(300), Degrees(f64::NAN));
    assert!(matches!(
        it.next(),
        Some(Err(Error::NonFiniteArgumentOfLatitude(_)))
    ));
    assert!(it.next().is_none());
}

#[test]
fn test_equatorial_orbit_has_no_ascending_node() {
    let tle = common::create_tle();
    let mut elements =
        Elements::from_tle(None, tle.line_1().as_bytes(), tle.line_2().as_bytes()).unwrap();
    elements.inclination = 0.0;
    let p = Predictor::new(elements).unwrap();
    assert_eq!(
        p.argument_of_latitude(p.epoch()),
        Err(Error::NoAscendingNode)
    );
    let start = common::datetime("2025-12-20T12:00:00Z");
    let first = p
        .arg_lat_iter(start..start + Duration::minutes(10), Degrees(0.0))
        .next();
    assert_eq!(first, Some(Err(Error::NoAscendingNode)));
}
