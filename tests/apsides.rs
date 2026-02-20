mod common;

use chrono::Duration;
use sgp4_predict::{ApsisEvent, Predictor};

#[test]
fn test_apsides() {
    let tle = common::create_tle();
    let p = Predictor::new(&tle).unwrap();

    // Two orbital periods for Sentinel-2C (~100 min/orbit → ~200 min)
    let start_dt = common::datetime("2025-12-20T12:00:00Z");
    let end_dt = start_dt + Duration::minutes(200);

    let apsides = p
        .apsis_iter(start_dt..end_dt)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert!(!apsides.is_empty(), "expected at least one apsis event");

    // Verify that events strictly alternate
    for window in apsides.windows(2) {
        assert_ne!(
            window[0].event, window[1].event,
            "consecutive apsis events must alternate between Perigee and Apogee"
        );
    }

    // Verify both event types are present
    assert!(
        apsides.iter().any(|a| a.event == ApsisEvent::Perigee),
        "expected at least one perigee"
    );
    assert!(
        apsides.iter().any(|a| a.event == ApsisEvent::Apogee),
        "expected at least one apogee"
    );
}

#[test]
#[ignore]
fn test_apsides_to_csv() {
    use std::io::Write;

    let tle = common::create_tle();
    let sat_id = tle.satellite.clone();
    let p = Predictor::new(&tle).unwrap();

    let start_dt = common::datetime("2025-12-20T12:00:00Z");
    let end_dt = common::datetime("2025-12-23T12:00:00Z");

    let start_str = start_dt.format("%Y%m%dT%H%M%S").to_string();
    let end_str = end_dt.format("%Y%m%dT%H%M%S").to_string();
    let filename = format!("{}_apsides_{}_{}.csv", sat_id, start_str, end_str);

    std::fs::create_dir_all("tests/results").unwrap();
    let filepath = format!("tests/results/{}", filename);

    let mut file = std::fs::File::create(&filepath).unwrap();
    writeln!(file, "time,event").unwrap();

    let mut count = 0;
    for apsis in p.apsis_iter(start_dt..end_dt) {
        let apsis = apsis.unwrap();
        let event_str = match apsis.event {
            ApsisEvent::Perigee => "perigee",
            ApsisEvent::Apogee => "apogee",
        };
        writeln!(
            file,
            "{},{}",
            apsis.time.format("%Y-%m-%d %H:%M:%S"),
            event_str
        )
        .unwrap();
        count += 1;
    }

    println!("Wrote {} apsis events to {}", count, filepath);
}
