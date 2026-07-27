use std::process::{Command, Output};

const TLE_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sentinel-2c.tle");
const CONFIG_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/config.yaml");

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args(args)
        .args(["--tle-file", TLE_FILE, "--config", CONFIG_FILE])
        .args(["--start", "2025-12-22T12:00:00Z", "--duration", "6h"])
        .output()
        .expect("failed to run sgp4-predict")
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn test_text_header_underline_matches() {
    let out = stdout(&run(&["transits", "--gs", "glasgow"]));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0].chars().count(), lines[1].chars().count());
    assert!(lines[1].chars().all(|c| c == '-'));
}

#[test]
fn test_json_rows_are_valid_ndjson() {
    let out = stdout(&run(&["transits", "--gs", "glasgow", "--format", "json"]));
    assert!(!out.trim().is_empty(), "expected at least one transit");
    for line in out.lines() {
        assert!(line.starts_with('{') && line.ends_with('}'), "{line}");
        // Every key is quoted and every row carries all seven columns.
        assert_eq!(line.matches("\":").count(), 7, "{line}");
    }
}

#[test]
fn test_json_numbers_are_unquoted() {
    let out = stdout(&run(&["transits", "--gs", "glasgow", "--format", "json"]));
    let line = out.lines().next().unwrap();
    assert!(line.contains("\"aos\":\""), "times are strings: {line}");
    assert!(
        !line.contains("\"tca_el_deg\":\""),
        "numbers must not be quoted: {line}"
    );
}

#[test]
fn test_csv_header_and_column_count() {
    let out = stdout(&run(&["transits", "--gs", "glasgow", "--format", "csv"]));
    let mut lines = out.lines();
    assert_eq!(
        lines.next().unwrap(),
        "aos,los,aos_az_deg,los_az_deg,tca_time,tca_el_deg,duration"
    );
    for line in lines {
        assert_eq!(line.split(',').count(), 7, "{line}");
    }
}

/// Every subcommand must honour --format, not just transits.
#[test]
fn test_all_subcommands_support_formats() {
    for command in ["apsides", "illumination"] {
        let csv = stdout(&run(&[command, "--format", "csv"]));
        assert!(
            !csv.lines().next().unwrap().contains(' '),
            "{command}: {csv}"
        );

        let json = stdout(&run(&[command, "--format", "json"]));
        for line in json.lines() {
            assert!(line.starts_with('{'), "{command}: {line}");
        }
    }

    let csv = stdout(&run(&[
        "observations",
        "--gs",
        "glasgow",
        "--format",
        "csv",
    ]));
    assert!(csv.starts_with("datetime,az_deg,el_deg,range_km,range_rate_km_s\n"));

    let csv = stdout(&run(&[
        "state-vectors",
        "--format",
        "csv",
        "--frame",
        "ecef",
    ]));
    assert!(csv.starts_with("datetime,x_km,y_km,z_km,vx_km_s,vy_km_s,vz_km_s\n"));
}

/// An empty result set still identifies its columns in text and CSV.
#[test]
fn test_empty_result_still_has_header() {
    let out = stdout(&run(&[
        "transits",
        "--gs",
        "glasgow",
        "--min-elevation",
        "89",
    ]));
    assert_eq!(out.lines().count(), 2, "{out:?}");
}

/// `#` comment lines would make JSON output unparseable.
#[test]
fn test_output_args_rejected_for_json() {
    let out = run(&[
        "transits",
        "--gs",
        "glasgow",
        "--format",
        "json",
        "--output-args",
    ]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("--output-args is only supported with --format text"),
        "{stderr}"
    );
}

#[test]
fn test_step_zero_is_rejected() {
    let out = run(&["observations", "--gs", "glasgow", "--step", "0s"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("step must be greater than zero"),
        "{stderr}"
    );
}

#[test]
fn test_min_elevation_range_is_enforced() {
    for bad in ["500", "-91"] {
        let out = run(&["transits", "--gs", "glasgow", "--min-elevation", bad]);
        assert!(!out.status.success(), "{bad} was accepted");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("elevation must be in [-90, 90]"),
            "{stderr}"
        );
    }
}

/// Negative elevations must parse without needing `--min-elevation=-5`.
#[test]
fn test_negative_min_elevation_parses_as_separate_arg() {
    let out = run(&["transits", "--gs", "glasgow", "--min-elevation", "-5"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn test_config_is_accepted_before_the_subcommand() {
    // --config is global, so it may appear either side of the subcommand.
    let out = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args(["--config", CONFIG_FILE, "transits", "--gs", "glasgow"])
        .args(["--tle-file", TLE_FILE, "--duration", "2h"])
        .output()
        .expect("failed to run sgp4-predict");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn test_completions_and_man_generate() {
    for shell in ["bash", "zsh", "fish"] {
        let out = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
            .args(["completions", shell])
            .output()
            .expect("failed to run sgp4-predict");
        let script = stdout(&out);
        assert!(script.contains("sgp4-predict"), "{shell} completions empty");
    }

    let out = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .arg("man")
        .output()
        .expect("failed to run sgp4-predict");
    let page = stdout(&out);
    assert!(page.contains(".TH"), "not a roff man page");
}

/// clap_complete panics on write errors, so the script must be buffered before
/// it reaches a possibly-closed stdout.
#[test]
fn test_completions_survive_a_closed_pipe() {
    use std::process::Stdio;

    let mut child = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args(["completions", "fish"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run sgp4-predict");

    drop(child.stdout.take());
    let out = child.wait_with_output().expect("failed to collect output");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
}

/// --quiet suppresses the stale-TLE warning; the default does not.
/// The start is well past the TLE epoch (day 356 of 2025) so the warning fires.
fn apsides_long_after_epoch(extra: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args(extra)
        .args(["apsides", "--tle-file", TLE_FILE])
        .args(["--start", "2026-06-01T00:00:00Z", "--duration", "2h"])
        .env_remove("RUST_LOG")
        .output()
        .expect("failed to run sgp4-predict")
}

#[test]
fn test_quiet_suppresses_warnings() {
    let noisy = apsides_long_after_epoch(&[]);
    assert!(
        String::from_utf8_lossy(&noisy.stderr).contains("days old"),
        "expected a stale-TLE warning by default"
    );

    let quiet = apsides_long_after_epoch(&["--quiet"]);
    assert_eq!(String::from_utf8_lossy(&quiet.stderr), "");
}

#[test]
fn test_verbose_adds_info_logging() {
    let plain = apsides_long_after_epoch(&[]);
    assert!(!String::from_utf8_lossy(&plain.stderr).contains("predictor ready"));

    let verbose = apsides_long_after_epoch(&["-v"]);
    assert!(
        String::from_utf8_lossy(&verbose.stderr).contains("predictor ready"),
        "{}",
        String::from_utf8_lossy(&verbose.stderr)
    );
}
