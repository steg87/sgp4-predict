mod common;

use chrono::Duration;
use sgp4_predict::Predictor;

#[test]
fn test_propagate() {
    let tle = common::create_tle();
    let p = Predictor::new(&tle);
    let predictions = p
        .prediction_iter(
            &(common::datetime("2025-12-20T12:00:00Z")..common::datetime("2025-12-23T12:00:00Z")),
            Duration::minutes(1),
        )
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!predictions.is_empty());
}
