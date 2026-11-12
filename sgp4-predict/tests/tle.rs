mod common;

use sgp4_predict::{Error, Predictor, Tle};

#[test]
fn test_predictor_parses_from_str() {
    assert!(matches!(
        "not a tle".parse::<Predictor>(),
        Err(Error::TleFormat(_))
    ));
}

#[test]
fn test_two_line_tle_is_named_by_norad_id() {
    let tle = common::create_tle();
    let two_line = format!("{}\n{}", tle.line_1, tle.line_2);
    let parsed: Tle = two_line.parse().unwrap();
    assert_eq!(parsed.satellite_name, "60989");
    assert!(two_line.parse::<Predictor>().is_ok());
}
