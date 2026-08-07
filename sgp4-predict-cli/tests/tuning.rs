//! The detection-tuning and root-finder flags, end to end.
//!
//! `src/tuning.rs` unit-tests the mapping itself — that every flag default
//! equals the library's `Default`, and that each value lands in the right
//! field. This file only checks what that cannot: that the values reach the
//! iterators at all, and that the header reports them.

use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn tle() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sentinel-2c.tle")
}

fn config(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).expect("failed to create tmpdir");
    let path = dir.join("tuning.yaml");
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
",
    )
    .expect("failed to write config");
    path
}

fn run(config: &Path, command: &str, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args([command, "--start", "2025-12-22 12:00:00"])
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

/// Each header line is spelled as the flag that sets it, so a recorded run can
/// be replayed by pasting its own header back onto the command line.
#[test]
fn test_header_records_every_knob() {
    let config = config("tuning_header");
    let stdout = ok(&run(
        &config,
        "aoi-windows",
        &["--aoi", "europe", "--duration", "1d", "--output-args"],
    ));
    for line in [
        "# min-step: 1s",
        "# max-step: 10m",
        "# walk-step: 5s",
        "# max-window-duration: 1h",
        "# skip-leading-partial: true",
        "# clamp-to-interval: false",
        "# time-tolerance: 0.001",
        "# max-iter: 100",
    ] {
        assert!(stdout.contains(line), "missing {line:?} in:\n{stdout}");
    }
}

/// Both directions of the cap on one AOI: below its window length it errors,
/// above it succeeds. This is the knob that was previously unreachable.
#[test]
fn test_max_window_duration_binds_and_can_be_raised() {
    let config = config("tuning_window_cap");
    fn args(cap: &str) -> Vec<&str> {
        vec![
            "--aoi",
            "europe",
            "--duration",
            "1d",
            "--max-window-duration",
            cap,
        ]
    }

    let message = err(&run(&config, "aoi-windows", &args("30s")));
    assert!(message.contains("window"), "{message}");

    let stdout = ok(&run(&config, "aoi-windows", &args("3h")));
    assert!(stdout.lines().count() > 2, "{stdout}");
}

#[test]
fn test_zero_and_negative_values_are_rejected() {
    let config = config("tuning_rejects");
    for (command, extra, flag, value, wanted) in [
        (
            "aoi-windows",
            vec!["--aoi", "europe"],
            "--min-step",
            "0s",
            "step must be greater than zero",
        ),
        (
            "aoi-windows",
            vec!["--aoi", "europe"],
            "--max-window-duration",
            "0s",
            "duration must be greater than zero",
        ),
        (
            "apsides",
            vec![],
            "--time-tolerance",
            "0",
            "time tolerance must be greater than zero",
        ),
        (
            "apsides",
            vec![],
            "--max-iter",
            "0",
            "max iterations must be at least 1",
        ),
    ] {
        let mut args = extra;
        args.extend([flag, value]);
        let message = err(&run(&config, command, &args));
        assert!(
            message.contains(wanted),
            "{command} {flag} {value}: {message}"
        );
    }
}
