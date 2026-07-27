use std::{
    io::Write as _,
    process::{Command, Output, Stdio},
};

const CONFIG_FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/config.yaml");
const LINE1: &str = "1 60989U 24157A   25356.66913557  .00000141  00000+0  70244-4 0  9990";
const LINE2: &str = "2 60989  98.5671  69.0082 0001197  95.1447 264.9872 14.30821394 67740";

/// Run `transits` with no --tle-file, feeding `stdin` to the process.
fn transits_with_stdin(stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args([
            "transits",
            "--start",
            "2025-12-22 12:00:00",
            "--duration",
            "1d",
        ])
        .args(["--config", CONFIG_FILE, "--gs", "glasgow"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run sgp4-predict");

    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(stdin.as_bytes())
        .expect("failed to write TLE to stdin");

    child.wait_with_output().expect("failed to collect output")
}

#[test]
fn test_three_line_tle_from_stdin() {
    let out = transits_with_stdin(&format!("SENTINEL-2C\n{LINE1}\n{LINE2}\n"));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!String::from_utf8_lossy(&out.stdout).trim().is_empty());
}

/// Piped stdin must produce exactly what --tle-file produces.
#[test]
fn test_stdin_matches_tle_file() {
    let tle_file = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sentinel-2c.tle");
    let piped = transits_with_stdin(&std::fs::read_to_string(tle_file).unwrap());

    let from_file = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args([
            "transits",
            "--start",
            "2025-12-22 12:00:00",
            "--duration",
            "1d",
        ])
        .args(["--config", CONFIG_FILE, "--gs", "glasgow"])
        .args(["--tle-file", tle_file])
        .output()
        .expect("failed to run sgp4-predict");

    assert!(piped.status.success());
    assert!(from_file.status.success());
    assert_eq!(piped.stdout, from_file.stdout);
}

/// A 2-line TLE gets its name derived from the NORAD id.
#[test]
fn test_two_line_tle_from_stdin() {
    let out = transits_with_stdin(&format!("{LINE1}\n{LINE2}\n"));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn test_two_line_tle_derives_norad_name() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args([
            "apsides",
            "--start",
            "2025-12-22 12:00:00",
            "--duration",
            "2h",
        ])
        .arg("--output-args")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to run sgp4-predict");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(format!("{LINE1}\n{LINE2}\n").as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("# satellite: NORAD-60989"), "{stdout}");
}

#[test]
fn test_empty_stdin_errors() {
    let out = transits_with_stdin("");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("found 0"), "{stderr}");
}

#[test]
fn test_malformed_stdin_errors() {
    let out = transits_with_stdin("just one line\n");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("in TLE from stdin"), "{stderr}");
    assert!(stderr.contains("found 1"), "{stderr}");
}
