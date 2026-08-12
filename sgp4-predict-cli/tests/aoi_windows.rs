use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn tle() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sentinel-2c.tle")
}

/// A config seeded with one AOI of each shape, written directly rather than
/// through `aoi add` so this file tests only the prediction command.
fn config(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).expect("failed to create tmpdir");
    let path = dir.join("aois.yaml");
    std::fs::write(
        &path,
        r"
aois:
  europe:
    shape: box
    south: 40.0
    north: 65.0
    west: -10.0
    east: 30.0
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
    for id in ["europe", "europe-circle", "europe-poly"] {
        let stdout = ok(&run(&config, &["--aoi", id, "--duration", "1d"]));
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
        &["--aoi", "europe", "--duration", "1d", "--format", "csv"],
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
        &["--aoi", "europe", "--duration", "1d", "--format", "json"],
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

/// The header records the AOI by id and by its stored fields, so a saved run
/// is reproducible even if the config later changes.
#[test]
fn test_output_args_header_records_the_aoi() {
    let config = config("aoi_output_args");
    let stdout = ok(&run(
        &config,
        &["--aoi", "europe", "--duration", "1d", "--output-args"],
    ));
    assert!(stdout.contains("# command: aoi-windows"), "{stdout}");
    assert!(stdout.contains("# aoi: europe"), "{stdout}");
    assert!(stdout.contains("# aoi-shape: box"), "{stdout}");
    assert!(
        stdout.contains("# aoi-definition: south=40 north=65 west=-10 east=30"),
        "{stdout}"
    );
}

#[test]
fn test_an_aoi_that_is_never_overflown_yields_no_windows() {
    let config = config("aoi_empty");
    let stdout = ok(&run(&config, &["--aoi", "pacific", "--duration", "1d"]));
    // The header still identifies the columns.
    assert_eq!(stdout.lines().count(), 2, "{stdout}");
}

#[test]
fn test_missing_aoi_lists_known_ids() {
    let config = config("aoi_missing_id");
    let message = err(&run(&config, &["--duration", "1d"]));
    assert!(message.contains("--aoi is required"), "{message}");
    assert!(message.contains("europe"), "{message}");
}

#[test]
fn test_unknown_aoi_lists_known_ids() {
    let config = config("aoi_unknown_id");
    let message = err(&run(&config, &["--aoi", "nowhere", "--duration", "1d"]));
    assert!(message.contains("unknown aoi 'nowhere'"), "{message}");
    assert!(message.contains("known ids: europe"), "{message}");
}

/// A hand-edited AOI the library cannot build must name itself, not fail
/// somewhere anonymous inside the scan.
#[test]
fn test_unbuildable_aoi_names_itself() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("aoi_unbuildable");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("aois.yaml");
    std::fs::write(
        &path,
        r"
aois:
  broken:
    shape: circle
    latitude: 0.0
    longitude: 0.0
    radius: 95.0
",
    )
    .unwrap();

    let message = err(&run(&path, &["--aoi", "broken", "--duration", "1d"]));
    assert!(message.contains("aoi 'broken'"), "{message}");
    assert!(message.contains("radius must lie in (0, 90°)"), "{message}");
}

/// The AOI is resolved before the TLE, so a bad id fails without waiting on
/// stdin for a TLE that will never be used.
#[test]
fn test_unknown_aoi_fails_before_reading_the_tle() {
    let config = config("aoi_before_tle");
    let out = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args([
            "aoi-windows",
            "--start",
            "2025-12-22 12:00:00",
            "--duration",
            "1d",
        ])
        .args(["--config", config.to_str().unwrap(), "--aoi", "nowhere"])
        .args(["--tle-file", "/nonexistent/does-not-exist.tle"])
        .output()
        .expect("failed to run sgp4-predict");

    let message = String::from_utf8_lossy(&out.stderr);
    assert!(message.contains("unknown aoi"), "{message}");
    assert!(!message.contains("does-not-exist"), "{message}");
}

/// The field of regard reaches the detection rather than merely parsing.
///
/// Asserts that each nadir-only window is contained in a *strictly* wider one,
/// not that the window count rose: a count comparison passes when
/// `--max-off-nadir` is ignored entirely, since a wider cone merges windows as
/// readily as it adds them.
#[test]
fn test_max_off_nadir_widens_each_window() {
    let config = config("aoi_off_nadir");
    let windows = |args: &[&str]| {
        ok(&run(&config, args))
            .lines()
            .skip(1)
            .map(|row| {
                let mut f = row.split(',');
                let entry = f.next().expect("entry").to_string();
                let exit = f.next().expect("exit").to_string();
                (entry, exit)
            })
            .collect::<Vec<_>>()
    };

    let base: &[&str] = &[
        "--aoi",
        "europe-circle",
        "--duration",
        "1d",
        "--format",
        "csv",
    ];
    fn with<'a>(base: &[&'a str], extra: &[&'a str]) -> Vec<&'a str> {
        [base, extra].concat()
    }

    let nadir = windows(base);
    let wide = windows(&with(base, &["--max-off-nadir", "30"]));
    assert!(!nadir.is_empty(), "nadir-only found nothing");

    for (entry, exit) in &nadir {
        // Timestamps are fixed-width ISO 8601, so string order is time order.
        let containing = wide
            .iter()
            .find(|(s, e)| s <= entry && e >= exit)
            .unwrap_or_else(|| panic!("no 30° window contains {entry}..{exit}; got {wide:?}"));
        assert!(
            containing.0 < *entry && containing.1 > *exit,
            "30° window {containing:?} is no wider than the nadir window {entry}..{exit}"
        );
    }

    // Full coverage is strictly harder than any, at the same reach.
    let full = windows(&with(
        base,
        &["--max-off-nadir", "30", "--coverage", "full"],
    ));
    assert!(
        full.len() < wide.len(),
        "full ({}) should be under any ({})",
        full.len(),
        wide.len()
    );
}

/// A non-finite field of regard survives every clamp downstream and would be
/// read as full line-of-sight reach, so it is refused at the flag.
#[test]
fn test_non_finite_off_nadir_is_rejected() {
    let config = config("aoi_bad_off_nadir");
    for bad in ["nan", "inf", "-1", "90", "120"] {
        // `=` because clap reads a bare negative as a flag.
        let flag = format!("--max-off-nadir={bad}");
        let message = err(&run(
            &config,
            &["--aoi", "europe", "--duration", "1d", &flag],
        ));
        assert!(
            message.contains("off-nadir angle must be in [0, 90) degrees"),
            "{bad}: {message}"
        );
    }
}

/// A zero-width reach cannot contain an area, so this combination yields an
/// empty table. Warn rather than let it read as "no passes".
#[test]
fn test_full_coverage_without_a_field_of_regard_warns() {
    let config = config("aoi_full_zero");
    let out = run(
        &config,
        &["--aoi", "europe", "--duration", "1d", "--coverage", "full"],
    );
    ok(&out);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(stderr.contains("can never open a window"), "{stderr}");
}
