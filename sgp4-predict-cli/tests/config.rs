use std::{path::PathBuf, process::Command};

const TLE_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sentinel-2c.tle");
const CONFIG_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/config.yaml");

/// Write `contents` to a uniquely named file under the integration-test tmpdir.
fn write_config(name: &str, contents: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.yaml"));
    std::fs::write(&path, contents).unwrap();
    path
}

fn transits(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args([
            "transits",
            "--start",
            "2025-12-22 12:00:00",
            "--duration",
            "1d",
        ])
        .args(["--tle-file", TLE_FILE])
        .args(args)
        .output()
        .expect("failed to run sgp4-predict")
}

#[test]
fn test_gs_from_config() {
    let out = transits(&["--config", CONFIG_FILE, "--gs", "glasgow"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn test_output_args_records_gs_and_location() {
    let out = transits(&["--config", CONFIG_FILE, "--gs", "glasgow", "--output-args"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("# ground-station: glasgow"), "{stdout}");
    assert!(stdout.contains("# observer: 55.86,-4.25,10"), "{stdout}");
}

/// Altitude is optional in the config and defaults to 0.
#[test]
fn test_altitude_defaults_to_zero() {
    let out = transits(&["--config", CONFIG_FILE, "--gs", "svalbard", "--output-args"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("# observer: 78.23,15.39,0"), "{stdout}");
}

#[test]
fn test_missing_gs_errors() {
    let out = transits(&["--config", CONFIG_FILE]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("--gs is required"), "{stderr}");
    assert!(stderr.contains("glasgow, svalbard"), "{stderr}");
}

#[test]
fn test_unknown_gs_id_lists_known_ids() {
    let out = transits(&["--config", CONFIG_FILE, "--gs", "nowhere"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown ground station 'nowhere'"),
        "{stderr}"
    );
    assert!(stderr.contains("glasgow, svalbard"), "{stderr}");
}

#[test]
fn test_missing_explicit_config_errors() {
    let out = transits(&[
        "--config",
        "/nonexistent/sgp4-predict.yaml",
        "--gs",
        "glasgow",
    ]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("failed to read config file"), "{stderr}");
}

#[test]
fn test_malformed_config_errors() {
    let config = write_config("malformed", "groundstations: [not, a, map]\n");
    let out = transits(&["--config", config.to_str().unwrap(), "--gs", "glasgow"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("failed to parse config file"), "{stderr}");
}

#[test]
fn test_unknown_config_field_errors() {
    let config = write_config(
        "unknown_field",
        "groundstations:\n  glasgow:\n    location: { latitude: 55.86, longitude: -4.25 }\n    antenna: dish\n",
    );
    let out = transits(&["--config", config.to_str().unwrap(), "--gs", "glasgow"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("antenna"), "{stderr}");
}

#[test]
fn test_out_of_range_latitude_errors() {
    let config = write_config(
        "bad_latitude",
        "groundstations:\n  bad:\n    location: { latitude: 91.0, longitude: 0.0 }\n",
    );
    let out = transits(&["--config", config.to_str().unwrap(), "--gs", "bad"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("latitude must be in [-90, 90]"), "{stderr}");
}
