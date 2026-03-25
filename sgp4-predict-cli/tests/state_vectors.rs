use std::process::Command;

#[test]
fn test_state_vectors_teme() {
    let tle_file = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sentinel-2c.tle");

    let status = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args([
            "state-vectors",
            "--start",
            "2025-12-22 12:00:00",
            "--duration",
            "1h",
            "--step",
            "60s",
            "--tle-file",
            tle_file,
            "--frame",
            "teme",
        ])
        .status()
        .expect("failed to run sgp4-predict");

    assert!(status.success());
}

#[test]
fn test_state_vectors_ecef() {
    let tle_file = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sentinel-2c.tle");

    let status = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args([
            "state-vectors",
            "--start",
            "2025-12-22 12:00:00",
            "--duration",
            "1h",
            "--step",
            "60s",
            "--tle-file",
            tle_file,
            "--frame",
            "ecef",
        ])
        .status()
        .expect("failed to run sgp4-predict");

    assert!(status.success());
}
