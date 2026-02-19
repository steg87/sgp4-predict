mod common;

use sgp4_predict::Predictor;

#[test]
fn test_transits() {
    let tle = common::create_tle();
    let p = Predictor::new(&tle);
    let gs = common::GroundStation::new(55.8642, -4.2518, 40.0);
    let transits = p
        .transits_iter(
            &gs,
            common::datetime("2025-12-20T12:00:00Z")..common::datetime("2026-01-21T12:00:00Z"),
            0.0,
        )
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!transits.is_empty());
}

#[test]
#[ignore]
fn test_transits_to_csv() {
    use std::io::Write;

    let tle = common::create_tle();
    let sat_id = tle.satellite.clone();
    let p = Predictor::new(&tle);
    let gs = common::GroundStation::new(55.8642, -4.2518, 40.0);

    let start_dt = common::datetime("2025-12-20T12:00:00Z");
    let end_dt = common::datetime("2025-12-23T12:00:00Z");

    let start_str = start_dt.format("%Y%m%dT%H%M%S").to_string();
    let end_str = end_dt.format("%Y%m%dT%H%M%S").to_string();
    let filename = format!("{}_transits_{}_{}.csv", sat_id, start_str, end_str);

    std::fs::create_dir_all("tests/results").unwrap();
    let filepath = format!("tests/results/{}", filename);

    let mut file = std::fs::File::create(&filepath).unwrap();
    writeln!(file, "start,end,aos_azimuth_deg,los_azimuth_deg,duration").unwrap();

    let mut count = 0;
    for transit in p.transits_iter(&gs, start_dt..end_dt, 0.0) {
        let transit = transit.unwrap();

        let obs_start = p.observe_at(transit.start, &gs).unwrap();
        let obs_end = p.observe_at(transit.end, &gs).unwrap();

        let duration = transit.end - transit.start;
        let duration_str = humantime::format_duration(std::time::Duration::from_secs_f32(
            duration.as_seconds_f32().round(),
        ))
        .to_string();

        writeln!(
            file,
            "{},{},{:.2},{:.2},{}",
            transit.start.format("%Y-%m-%d %H:%M:%S"),
            transit.end.format("%Y-%m-%d %H:%M:%S"),
            obs_start.azimuth.to_degrees(),
            obs_end.azimuth.to_degrees(),
            duration_str
        )
        .unwrap();
        count += 1;
    }

    println!("Wrote {} transits to {}", count, filepath);
}
