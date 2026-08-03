use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn tle() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sentinel-2c.tle")
}

/// A config seeded with one area of each shape, written directly rather than
/// through `area add` so this file tests only the prediction command.
fn config(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).expect("failed to create tmpdir");
    let path = dir.join("areas.yaml");
    std::fs::write(
        &path,
        r"
areas:
  europe:
    shape: box
    latitude: 52.5
    longitude: 10.0
    width: 40.0
    height: 25.0
  europe-ellipse:
    shape: ellipse
    latitude: 52.0
    longitude: 10.0
    semi_major: 14.0
    semi_minor: 4.0
    bearing: 60.0
  europe-circle:
    shape: circle
    latitude: 52.0
    longitude: 10.0
    radius: 10.0
  europe-poly:
    shape: polygon
    vertices:
      - { latitude: 40.0, longitude: -10.0 }
      - { latitude: 40.0, longitude: 30.0 }
      - { latitude: 65.0, longitude: 30.0 }
      - { latitude: 65.0, longitude: -10.0 }
  pacific:
    shape: circle
    latitude: -40.0
    longitude: -140.0
    radius: 0.5
",
    )
    .expect("failed to write config");
    path
}

fn run(config: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args(["aoi-windows", "--start", "2025-12-22 12:00:00"])
        .args(["--config", config.to_str().unwrap()])
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

fn err(out: &Output) -> String {
    assert!(!out.status.success(), "expected a failure");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn test_aoi_cli_over_every_shape() {
    let config = config("aoi_shapes");
    for id in ["europe", "europe-ellipse", "europe-circle", "europe-poly"] {
        let stdout = ok(&run(&config, &["--area", id, "--duration", "1d"]));
        assert!(stdout.contains("entry"), "{id}: {stdout}");
        assert!(
            stdout.lines().count() > 2,
            "{id} should be overflown at least once:\n{stdout}"
        );
    }
}

/// The reported entry and exit points are on the box's own boundary, which is
/// the end-to-end check that the window times line up with the geometry.
#[test]
fn test_entry_and_exit_points_lie_on_the_boundary() {
    let config = config("aoi_boundary");
    let stdout = ok(&run(
        &config,
        &["--area", "europe", "--duration", "1d", "--format", "csv"],
    ));

    let mut rows = 0;
    for line in stdout.lines().skip(1) {
        let f: Vec<f64> = line
            .split(',')
            .skip(2)
            .take(4)
            .map(|v| v.parse().unwrap())
            .collect();
        let (entry_lat, entry_lon, exit_lat, exit_lon) = (f[0], f[1], f[2], f[3]);
        // The box spans 40..65 N by 10 W..30 E; each crossing sits on one edge.
        let on_edge = |lat: f64, lon: f64| {
            [40.0, 65.0].iter().any(|b: &f64| (lat - b).abs() < 1e-3)
                || [-10.0, 30.0].iter().any(|b: &f64| (lon - b).abs() < 1e-3)
        };
        assert!(
            on_edge(entry_lat, entry_lon),
            "entry off the boundary: {line}"
        );
        assert!(on_edge(exit_lat, exit_lon), "exit off the boundary: {line}");
        rows += 1;
    }
    assert!(rows > 0, "no windows found");
}

#[test]
fn test_aoi_json_fields() {
    let config = config("aoi_json");
    let stdout = ok(&run(
        &config,
        &["--area", "europe", "--duration", "1d", "--format", "json"],
    ));
    let row = stdout.lines().next().expect("at least one window");
    for key in [
        "entry",
        "exit",
        "entry_lat_deg",
        "entry_lon_deg",
        "exit_lat_deg",
        "exit_lon_deg",
        "duration",
    ] {
        assert!(row.contains(&format!("\"{key}\"")), "{row}");
    }
}

/// The header records the area by id and by its stored fields, so a saved run
/// is reproducible even if the config later changes.
#[test]
fn test_output_args_header_records_the_area() {
    let config = config("aoi_output_args");
    let stdout = ok(&run(
        &config,
        &["--area", "europe", "--duration", "1d", "--output-args"],
    ));
    assert!(stdout.contains("# command: aoi-windows"), "{stdout}");
    assert!(stdout.contains("# area: europe"), "{stdout}");
    assert!(stdout.contains("# area-shape: box"), "{stdout}");
    assert!(
        stdout.contains("# area-definition: latitude=52.5 longitude=10 width=40 height=25"),
        "{stdout}"
    );
}

#[test]
fn test_an_area_that_is_never_overflown_yields_no_windows() {
    let config = config("aoi_empty");
    let stdout = ok(&run(&config, &["--area", "pacific", "--duration", "1d"]));
    // The header still identifies the columns.
    assert_eq!(stdout.lines().count(), 2, "{stdout}");
}

#[test]
fn test_missing_area_lists_known_ids() {
    let config = config("aoi_missing_area");
    let message = err(&run(&config, &["--duration", "1d"]));
    assert!(message.contains("--area is required"), "{message}");
    assert!(message.contains("europe"), "{message}");
}

#[test]
fn test_unknown_area_lists_known_ids() {
    let config = config("aoi_unknown_area");
    let message = err(&run(&config, &["--area", "nowhere", "--duration", "1d"]));
    assert!(message.contains("unknown area 'nowhere'"), "{message}");
    assert!(message.contains("known ids: europe"), "{message}");
}

/// A hand-edited area the library cannot build must name itself, not fail
/// somewhere anonymous inside the scan.
#[test]
fn test_unbuildable_area_names_itself() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("aoi_unbuildable");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("areas.yaml");
    std::fs::write(
        &path,
        r"
areas:
  broken:
    shape: ellipse
    latitude: 0.0
    longitude: 0.0
    semi_major: 1.0
    semi_minor: 5.0
",
    )
    .unwrap();

    let message = err(&run(&path, &["--area", "broken", "--duration", "1d"]));
    assert!(message.contains("area 'broken'"), "{message}");
    assert!(message.contains("semi-minor"), "{message}");
}

/// The area is resolved before the TLE, so a bad id fails without waiting on
/// stdin for a TLE that will never be used.
#[test]
fn test_unknown_area_fails_before_reading_the_tle() {
    let config = config("aoi_area_before_tle");
    let out = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args([
            "aoi-windows",
            "--start",
            "2025-12-22 12:00:00",
            "--duration",
            "1d",
        ])
        .args(["--config", config.to_str().unwrap(), "--area", "nowhere"])
        .args(["--tle-file", "/nonexistent/does-not-exist.tle"])
        .output()
        .expect("failed to run sgp4-predict");

    let message = String::from_utf8_lossy(&out.stderr);
    assert!(message.contains("unknown area"), "{message}");
    assert!(!message.contains("does-not-exist"), "{message}");
}
