mod common;

use sgp4_predict::{Error, Predictor, Tle};

#[test]
fn test_predictor_from_str_rejects_malformed() {
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
    assert_eq!(parsed.satellite_name, "NORAD-60989");
    assert!(two_line.parse::<Predictor>().is_ok());
}

#[test]
fn test_two_line_tle_without_an_id_is_named_unknown() {
    let parsed: Tle = "1\n2".parse().unwrap();
    assert_eq!(parsed.satellite_name, "Unknown");
}

#[test]
fn test_three_line_tle_ignores_blank_lines_and_whitespace() {
    let tle = common::create_tle();
    let text = format!(
        "\n  SENTINEL-2C  \n\n  {}  \n{}\n\n",
        tle.line_1, tle.line_2
    );
    assert_eq!(text.parse::<Tle>().unwrap(), tle);
}

#[test]
fn test_tle_rejects_wrong_line_count() {
    let err = "only one line".parse::<Tle>().unwrap_err().to_string();
    assert!(err.contains("found 1"), "{err}");
}
