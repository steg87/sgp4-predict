mod common;

use chrono::Duration;
use sgp4_predict::{Degrees, Error, FallibleIter, GeodeticPoint, Predictor, Result};

fn err() -> Result<i32> {
    Err(Error::Custom("synthetic".into()))
}

fn predictor() -> Predictor {
    Predictor::from_tle(common::create_tle()).unwrap()
}

fn glasgow() -> GeodeticPoint {
    GeodeticPoint {
        latitude: Degrees(55.86),
        longitude: Degrees(-4.25),
        altitude: 40.0,
    }
}

// ---------------------------------------------------------------------------
// Skip-style adapters
// ---------------------------------------------------------------------------

/// `skip_errors` drops every `Err` and preserves the order of the rest.
#[test]
fn test_skip_errors_preserves_order() {
    let items = vec![err(), Ok(1), err(), Ok(2)];
    let values: Vec<i32> = items.into_iter().skip_errors().collect();
    assert_eq!(values, [1, 2], "expected the two Ok values in order");
}

/// An empty input yields nothing rather than blocking.
#[test]
fn test_skip_errors_on_empty_input() {
    let items: Vec<Result<i32>> = Vec::new();
    let values: Vec<i32> = items.into_iter().skip_errors().collect();
    assert!(values.is_empty(), "expected no values, got {values:?}");
}

/// An all-error input exhausts the inner iterator from inside the skip loop.
#[test]
fn test_skip_errors_on_all_error_input() {
    let items = vec![err(), err(), err()];
    let values: Vec<i32> = items.into_iter().skip_errors().collect();
    assert!(values.is_empty(), "expected no values, got {values:?}");
}

/// Both adapters can drop items, so neither may promise a lower bound.
#[test]
fn test_size_hint_has_no_lower_bound() {
    let items = vec![Ok(1), err(), Ok(2)];

    assert_eq!(
        items.clone().into_iter().skip_errors().size_hint(),
        (0, Some(3)),
        "OnError may yield anywhere from 0 to all 3 items"
    );
    assert_eq!(
        items.into_iter().tolerate_errors(1).size_hint(),
        (0, Some(3)),
        "Tolerate may yield anywhere from 0 to all 3 items"
    );
}

/// `on_error` invokes the handler once per error, in encounter order.
#[test]
fn test_on_error_sees_every_error_in_order() {
    let items = vec![Ok(1), Err(Error::Custom("first".into())), Ok(2), err()];
    let mut seen = Vec::new();

    let adapter = items.into_iter().on_error(|e| seen.push(e));
    // A closure handler must not cost the adapter its Debug impl.
    assert!(format!("{adapter:?}").starts_with("OnError"));

    let values: Vec<i32> = adapter.collect();

    assert_eq!(values, [1, 2], "handled errors must not end iteration");
    assert_eq!(
        seen,
        [
            Error::Custom("first".into()),
            Error::Custom("synthetic".into())
        ],
        "handler must see both errors in order"
    );
}

// ---------------------------------------------------------------------------
// Terminating adapters
// ---------------------------------------------------------------------------

/// `until_error` stops at the first error and retains it.
#[test]
fn test_until_error_stops_and_retains() {
    let items = vec![Ok(1), err(), Ok(2)];
    let mut it = items.into_iter().until_error();

    let values: Vec<i32> = it.by_ref().collect();

    assert_eq!(values, [1], "iteration must stop at the first error");
    assert_eq!(
        it.into_error(),
        Some(Error::Custom("synthetic".into())),
        "the terminating error must be retained"
    );
}

/// `tolerate_errors(2)` survives two consecutive errors and stops on the third.
#[test]
fn test_tolerate_errors_stops_past_the_limit() {
    let items = vec![Ok(1), err(), err(), Ok(2), err(), err(), err(), Ok(3)];
    let mut it = items.into_iter().tolerate_errors(2);

    let values: Vec<i32> = it.by_ref().collect();

    assert_eq!(values, [1, 2], "a run of three errors must end iteration");
    assert!(it.error().is_some(), "the terminating error must be stored");
}

/// A successful item resets the consecutive-error run.
#[test]
fn test_tolerate_errors_resets_run_on_ok() {
    let items = vec![err(), Ok(1), err(), Ok(2), err(), Ok(3)];
    let values: Vec<i32> = items.into_iter().tolerate_errors(1).collect();

    assert_eq!(values, [1, 2, 3], "isolated errors must all be tolerated");
}

/// An adapter that never trips yields everything and leaves no error behind.
#[test]
fn test_tolerate_errors_that_never_trips() {
    let items = vec![Ok(1), err(), Ok(2)];
    let mut it = items.into_iter().tolerate_errors(3);

    let values: Vec<i32> = it.by_ref().collect();

    assert_eq!(values, [1, 2]);
    assert!(it.error().is_none(), "no error should have terminated it");
}

/// Once stopped, further polls stay `None` — the inner iterator is not resumed.
#[test]
fn test_tolerate_errors_stays_stopped() {
    let items = vec![Ok(1), err(), Ok(2)];
    let mut it = items.into_iter().until_error();

    assert_eq!(it.next(), Some(1));
    assert_eq!(it.next(), None, "the error terminates iteration");
    assert_eq!(it.next(), None, "and it does not resume afterwards");
}

// ---------------------------------------------------------------------------
// Real iterators — the blanket impl must reach them
// ---------------------------------------------------------------------------

/// An owned crate iterator gains the adapters.
#[test]
fn test_applies_to_prediction_iter() {
    let predictor = predictor();
    let start = common::datetime("2025-12-20T12:00:00Z");
    let interval = start..start + Duration::minutes(10);

    let samples: Vec<_> = predictor
        .prediction_iter(interval, Duration::minutes(1))
        .log_errors()
        .collect();

    assert_eq!(samples.len(), 10, "10 minutes at a 1 minute cadence");
}

/// A borrowing, lifetime-parameterised iterator does too — and this one spells
/// its item `std::result::Result<_, Error>` rather than the crate alias.
#[test]
fn test_applies_to_observation_iter() {
    let predictor = predictor();
    let observer = glasgow();
    let start = common::datetime("2025-12-20T12:00:00Z");
    let interval = start..start + Duration::minutes(10);

    let mut it = predictor
        .observation_iter(observer, interval, Duration::minutes(1))
        .tolerate_errors(3);

    let samples: Vec<_> = it.by_ref().collect();

    assert_eq!(samples.len(), 10, "10 minutes at a 1 minute cadence");
    assert!(
        it.error().is_none(),
        "a valid TLE must not fail to propagate"
    );
}
