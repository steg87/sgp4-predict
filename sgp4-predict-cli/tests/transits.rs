use std::process::Command;

#[test]
fn test_transits_cli() {
    let tle_file = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sentinel-2c.tle");
    let config_file = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/config.yaml");

    let status = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args([
            "transits",
            "--start",
            "2025-12-22 12:00:00",
            "--duration",
            "1d",
            "--config",
            config_file,
            "--gs",
            "glasgow",
            "--tle-file",
            tle_file,
            "--min-elevation",
            "10",
        ])
        .status()
        .expect("failed to run sgp4-predict");

    assert!(status.success());
}
