use std::{
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

/// A fresh, non-existent config path under the integration-test tmpdir.
fn fresh_config(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("nested").join("stations.yaml")
}

fn gs(config: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args(["--config", config.to_str().unwrap(), "gs"])
        .args(args)
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
        .expect("failed to write stdin");
    child.wait_with_output().expect("failed to collect output")
}

fn ok(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// `gs add` prompts field by field and creates the file and its parents.
#[test]
fn test_add_creates_config_and_parents() {
    let config = fresh_config("gs_add");
    let out = gs(&config, &["add"], "glasgow\n55.86\n-4.25\n40\n");
    ok(&out);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("created"),
        "creating a config should be reported"
    );

    assert!(config.is_file(), "config was not created");
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(text.contains("glasgow"), "{text}");
    assert!(text.contains("55.86"), "{text}");

    let listed = ok(&gs(&config, &["list"], ""));
    assert!(listed.contains("glasgow"), "{listed}");
}

/// A blank altitude takes the documented default of 0.
#[test]
fn test_add_altitude_defaults_to_zero() {
    let config = fresh_config("gs_add_default_alt");
    ok(&gs(&config, &["add"], "svalbard\n78.23\n15.39\n\n"));

    let listed = ok(&gs(&config, &["list", "--format", "csv"], ""));
    assert!(listed.contains("svalbard,78.2300,15.3900,0.0"), "{listed}");
}

#[test]
fn test_add_rejects_duplicate_id() {
    let config = fresh_config("gs_add_dup");
    ok(&gs(&config, &["add"], "glasgow\n55.86\n-4.25\n40\n"));

    let out = gs(&config, &["add"], "glasgow\n1\n2\n3\n");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("already exists"), "{stderr}");
}

#[test]
fn test_add_rejects_out_of_range_latitude() {
    let config = fresh_config("gs_add_bad_lat");
    let out = gs(&config, &["add"], "bad\n91\n0\n0\n");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("latitude must be in [-90, 90]"), "{stderr}");
    assert!(!config.exists(), "a rejected station must not be written");
}

#[test]
fn test_add_rejects_non_numeric_latitude() {
    let config = fresh_config("gs_add_nan");
    let out = gs(&config, &["add"], "bad\nnorth\n0\n0\n");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("expected a number"), "{stderr}");
}

/// Only `gs add` may create a config; a missing --config is otherwise the
/// wrong path, not an empty station list.
#[test]
fn test_list_rejects_a_missing_explicit_config() {
    let config = fresh_config("gs_list_missing");
    let out = gs(&config, &["list"], "");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("does not exist"), "{stderr}");
    assert!(!config.exists(), "list must not create the config");
}

#[test]
fn test_remove_rejects_a_missing_explicit_config() {
    let config = fresh_config("gs_rm_missing");
    let out = gs(&config, &["remove", "glasgow", "--force"], "");
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("does not exist"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!config.exists());
}

/// An existing but empty config lists cleanly rather than erroring.
#[test]
fn test_list_is_empty_for_an_empty_config() {
    let config = fresh_config("gs_list_empty");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, "groundstations: {}\n").unwrap();

    let listed = ok(&gs(&config, &["list"], ""));
    // Header and underline only.
    assert_eq!(listed.lines().count(), 2, "{listed:?}");
}

#[test]
fn test_list_honours_format() {
    let config = fresh_config("gs_list_format");
    ok(&gs(&config, &["add"], "glasgow\n55.86\n-4.25\n40\n"));

    let csv = ok(&gs(&config, &["list", "--format", "csv"], ""));
    assert_eq!(
        csv.lines().next().unwrap(),
        "id,latitude,longitude,altitude"
    );

    let json = ok(&gs(&config, &["list", "--format", "json"], ""));
    assert!(
        json.starts_with('{') && json.contains("\"id\":\"glasgow\""),
        "{json}"
    );
}

#[test]
fn test_ls_and_rm_aliases_work() {
    let config = fresh_config("gs_aliases");
    ok(&gs(&config, &["add"], "glasgow\n55.86\n-4.25\n40\n"));

    let listed = ok(&gs(&config, &["ls"], ""));
    assert!(listed.contains("glasgow"), "{listed}");

    ok(&gs(&config, &["rm", "glasgow", "--force"], ""));
    let listed = ok(&gs(&config, &["ls"], ""));
    assert!(!listed.contains("glasgow"), "{listed}");
}

/// remove prints the station and asks before deleting.
#[test]
fn test_remove_confirms_before_deleting() {
    let config = fresh_config("gs_rm_confirm");
    ok(&gs(&config, &["add"], "glasgow\n55.86\n-4.25\n40\n"));

    let out = gs(&config, &["remove", "glasgow"], "y\n");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("latitude 55.86"), "{stderr}");
    assert!(
        stderr.contains("Remove ground station 'glasgow'?"),
        "{stderr}"
    );
    assert!(
        !std::fs::read_to_string(&config)
            .unwrap()
            .contains("glasgow")
    );
}

#[test]
fn test_remove_declined_leaves_config_untouched() {
    let config = fresh_config("gs_rm_declined");
    ok(&gs(&config, &["add"], "glasgow\n55.86\n-4.25\n40\n"));
    let before = std::fs::read_to_string(&config).unwrap();

    let out = gs(&config, &["remove", "glasgow"], "n\n");
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("aborted"));
    assert_eq!(std::fs::read_to_string(&config).unwrap(), before);
}

/// EOF on stdin must not be read as consent.
#[test]
fn test_remove_treats_eof_as_no() {
    let config = fresh_config("gs_rm_eof");
    ok(&gs(&config, &["add"], "glasgow\n55.86\n-4.25\n40\n"));

    let out = gs(&config, &["remove", "glasgow"], "");
    assert!(out.status.success());
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .contains("glasgow")
    );
}

#[test]
fn test_remove_force_skips_the_prompt() {
    let config = fresh_config("gs_rm_force");
    ok(&gs(&config, &["add"], "glasgow\n55.86\n-4.25\n40\n"));

    let out = gs(&config, &["remove", "glasgow", "-f"], "");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(!stderr.contains("Remove ground station"), "{stderr}");
}

#[test]
fn test_remove_unknown_id_lists_known_ids() {
    let config = fresh_config("gs_rm_unknown");
    ok(&gs(&config, &["add"], "glasgow\n55.86\n-4.25\n40\n"));

    let out = gs(&config, &["remove", "nowhere", "--force"], "");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("unknown ground station 'nowhere'"),
        "{stderr}"
    );
    assert!(stderr.contains("known ids: glasgow"), "{stderr}");
}

/// A station added via `gs add` must be usable by the prediction commands.
#[test]
fn test_added_station_works_for_transits() {
    let config = fresh_config("gs_end_to_end");
    ok(&gs(&config, &["add"], "glasgow\n55.86\n-4.25\n40\n"));

    let out = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args([
            "--config",
            config.to_str().unwrap(),
            "transits",
            "--gs",
            "glasgow",
        ])
        .args([
            "--tle-file",
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/sentinel-2c.tle"),
        ])
        .args(["--start", "2025-12-22T12:00:00Z", "--duration", "6h"])
        .output()
        .expect("failed to run sgp4-predict");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A malformed config must not be silently overwritten by an edit.
#[test]
fn test_add_refuses_to_clobber_a_broken_config() {
    let config = fresh_config("gs_broken");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, "groundstations: [not, a, map]\n").unwrap();

    let out = gs(&config, &["add"], "glasgow\n55.86\n-4.25\n40\n");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("failed to parse config file"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        "groundstations: [not, a, map]\n"
    );
}
