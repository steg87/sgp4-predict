use std::process::{Command, Output};

fn tle() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sentinel-2c.tle")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args(["ground-track", "--start", "2025-12-22 12:00:00"])
        .args(args)
        .args(["--tle-file", tle()])
        .output()
        .expect("failed to run sgp4-predict")
}

fn ok(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn test_ground_track_cli() {
    let stdout = ok(&run(&["--duration", "1h", "--step", "10m"]));
    assert!(stdout.contains("lat [deg]"), "{stdout}");
    assert!(stdout.contains("altitude [km]"), "{stdout}");
    // The interval is end-exclusive: 1h at 10m is six samples.
    assert_eq!(stdout.lines().count(), 2 + 6, "{stdout}");
}

#[test]
fn test_ground_track_json_fields() {
    let stdout = ok(&run(&[
        "--duration",
        "10m",
        "--step",
        "10m",
        "--format",
        "json",
    ]));
    let row = stdout.lines().next().expect("one row");
    for key in ["datetime", "lat_deg", "lon_deg", "altitude_km"] {
        assert!(row.contains(&format!("\"{key}\"")), "{row}");
    }
}

/// The sub-satellite point stays on the globe and at orbital altitude.
#[test]
fn test_ground_track_values_are_plausible() {
    let stdout = ok(&run(&[
        "--duration",
        "2h",
        "--step",
        "1m",
        "--format",
        "csv",
    ]));
    let mut rows = 0;
    for line in stdout.lines().skip(1) {
        let fields: Vec<&str> = line.split(',').collect();
        let lat: f64 = fields[1].parse().unwrap();
        let lon: f64 = fields[2].parse().unwrap();
        let altitude_km: f64 = fields[3].parse().unwrap();
        assert!((-90.0..=90.0).contains(&lat), "{line}");
        assert!((-180.0..=180.0).contains(&lon), "{line}");
        assert!((700.0..900.0).contains(&altitude_km), "{line}");
        rows += 1;
    }
    assert_eq!(rows, 120);
}

#[test]
fn test_zero_step_is_rejected() {
    let out = run(&["--duration", "1h", "--step", "0s"]);
    assert!(!out.status.success());
    let message = String::from_utf8_lossy(&out.stderr);
    assert!(message.contains("greater than zero"), "{message}");
}
