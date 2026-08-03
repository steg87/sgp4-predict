//! `sgp4-predict aoi add|remove|list`.

use std::{
    io::Write as _,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

/// A fresh, non-existent config path under the integration-test tmpdir.
fn fresh_config(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    dir.join("nested").join("areas.yaml")
}

fn aoi(config: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
        .args(["--config", config.to_str().unwrap(), "aoi"])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to run sgp4-predict");
    let _ = child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(stdin.as_bytes());
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

fn err(out: &Output) -> String {
    assert!(!out.status.success(), "expected a failure");
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Every shape reaches the config file as named YAML fields, not as the
/// positional flag syntax that produced it.
#[test]
fn test_add_stores_named_fields_for_every_shape() {
    let config = fresh_config("area_add_all_shapes");
    ok(&aoi(
        &config,
        &["add", "scotland", "--box", "57,-4.5,7,6"],
        "",
    ));
    ok(&aoi(
        &config,
        &["add", "north-sea", "--ellipse", "56,2,2.7,1.1,45"],
        "",
    ));
    ok(&aoi(
        &config,
        &["add", "cape-town", "--circle", "-33.9,18.4,2.25"],
        "",
    ));
    ok(&aoi(
        &config,
        &["add", "corridor", "--poly", "(54,-8),(54,-1),(60,-1)"],
        "",
    ));

    let text = std::fs::read_to_string(&config).unwrap();
    for expected in [
        "shape: box",
        "width: 7.0",
        "height: 6.0",
        "shape: ellipse",
        "semi_major: 2.7",
        "bearing: 45.0",
        "shape: circle",
        "radius: 2.25",
        "shape: polygon",
        "vertices:",
    ] {
        assert!(text.contains(expected), "missing {expected} in:\n{text}");
    }
    // Nothing comma-separated is persisted.
    assert!(!text.contains("57,-4.5"), "{text}");
}

#[test]
fn test_add_creates_config_and_parents() {
    let config = fresh_config("area_add_creates");
    let out = aoi(&config, &["add", "scotland", "--box", "57,-4.5,7,6"], "");
    ok(&out);
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("created"),
        "creating a config should be reported"
    );
    assert!(config.is_file(), "config was not created");

    let listed = ok(&aoi(&config, &["list"], ""));
    assert!(listed.contains("scotland"), "{listed}");
    assert!(listed.contains("box"), "{listed}");
}

/// The listing shows the config's own field names, so it reads like the YAML.
#[test]
fn test_list_shows_config_field_names() {
    let config = fresh_config("area_list_fields");
    ok(&aoi(
        &config,
        &["add", "scotland", "--box", "57,-4.5,7,6"],
        "",
    ));

    let listed = ok(&aoi(&config, &["list", "--format", "csv"], ""));
    assert!(
        listed.contains("scotland,box,latitude=57 longitude=-4.5 width=7 height=6"),
        "{listed}"
    );
}

#[test]
fn test_ls_and_rm_aliases() {
    let config = fresh_config("area_aliases");
    ok(&aoi(
        &config,
        &["add", "scotland", "--box", "57,-4.5,7,6"],
        "",
    ));
    assert!(ok(&aoi(&config, &["ls"], "")).contains("scotland"));

    ok(&aoi(&config, &["rm", "scotland", "--force"], ""));
    assert!(!ok(&aoi(&config, &["ls"], "")).contains("scotland"));
}

/// Anything but y/yes leaves the config alone, and so does EOF.
#[test]
fn test_remove_requires_confirmation() {
    let config = fresh_config("area_remove_confirm");
    ok(&aoi(
        &config,
        &["add", "scotland", "--box", "57,-4.5,7,6"],
        "",
    ));

    ok(&aoi(&config, &["remove", "scotland"], "n\n"));
    assert!(ok(&aoi(&config, &["list"], "")).contains("scotland"));

    // EOF: a non-interactive caller that forgot --force deletes nothing.
    ok(&aoi(&config, &["remove", "scotland"], ""));
    assert!(ok(&aoi(&config, &["list"], "")).contains("scotland"));

    ok(&aoi(&config, &["remove", "scotland"], "y\n"));
    assert!(!ok(&aoi(&config, &["list"], "")).contains("scotland"));
}

#[test]
fn test_remove_unknown_id_lists_known_ids() {
    let config = fresh_config("area_remove_unknown");
    ok(&aoi(
        &config,
        &["add", "scotland", "--box", "57,-4.5,7,6"],
        "",
    ));

    let message = err(&aoi(&config, &["remove", "nowhere", "--force"], ""));
    assert!(message.contains("unknown area 'nowhere'"), "{message}");
    assert!(message.contains("known ids: scotland"), "{message}");
}

/// A duplicate id needs --force, so an existing area is never silently
/// replaced.
#[test]
fn test_add_refuses_to_overwrite_without_force() {
    let config = fresh_config("area_add_duplicate");
    ok(&aoi(
        &config,
        &["add", "scotland", "--box", "57,-4.5,7,6"],
        "",
    ));

    let message = err(&aoi(&config, &["add", "scotland", "--circle", "0,0,1"], ""));
    assert!(message.contains("already exists"), "{message}");
    assert!(
        ok(&aoi(&config, &["list"], "")).contains("box"),
        "unchanged"
    );

    ok(&aoi(
        &config,
        &["add", "scotland", "--circle", "0,0,1", "--force"],
        "",
    ));
    assert!(ok(&aoi(&config, &["list"], "")).contains("circle"));
}

#[test]
fn test_shapes_are_mutually_exclusive() {
    let config = fresh_config("area_exclusive");
    let message = err(&aoi(
        &config,
        &["add", "x", "--box", "1,2,3,4", "--circle", "1,2,3"],
        "",
    ));
    assert!(message.contains("cannot be used with"), "{message}");
}

#[test]
fn test_a_shape_is_required() {
    let config = fresh_config("area_shape_required");
    let message = err(&aoi(&config, &["add", "x"], ""));
    assert!(
        message.contains("required arguments were not provided"),
        "{message}"
    );
}

/// Geometry the library rejects never reaches the file.
#[test]
fn test_invalid_geometry_is_rejected_before_saving() {
    let config = fresh_config("area_invalid_geometry");
    // semi-minor above semi-major.
    let message = err(&aoi(&config, &["add", "x", "--ellipse", "0,0,1,5"], ""));
    assert!(message.contains("semi-minor"), "{message}");
    assert!(
        !config.exists(),
        "a rejected area must not create the config"
    );

    // A box whose height runs past the pole.
    let message = err(&aoi(&config, &["add", "x", "--box", "88,0,10,10"], ""));
    assert!(message.contains("outside [-90, 90]"), "{message}");
}

#[test]
fn test_malformed_shape_values_name_the_flag() {
    let config = fresh_config("area_malformed");
    for (args, expected) in [
        (["add", "x", "--box", "1,2,3"], "expected LAT,LON,W,H"),
        (
            ["add", "x", "--box", "0,0,400,5"],
            "width must be in (0, 360)",
        ),
        (["add", "x", "--circle", "1,2"], "expected LAT,LON,R"),
        (["add", "x", "--ellipse", "1,2,3"], "expected LAT,LON,A,B"),
        (["add", "x", "--poly", "1,2,3,4"], "expected '('"),
        (["add", "x", "--poly", "(1,2),(3,4)"], "at least 3 vertices"),
        (
            ["add", "x", "--box", "1,2,x,4"],
            "expected a number, got 'x'",
        ),
    ] {
        let message = err(&aoi(&config, &args, ""));
        assert!(message.contains(expected), "{args:?}: {message}");
    }
}

/// Negative coordinates are values, not flags.
#[test]
fn test_leading_negative_coordinates_are_accepted() {
    let config = fresh_config("area_negative");
    ok(&aoi(
        &config,
        &["add", "cape", "--circle", "-33.9,18.4,2"],
        "",
    ));
    assert!(ok(&aoi(&config, &["list"], "")).contains("latitude=-33.9"));
}

/// `area list` and `area remove` must not create a config the user pointed at
/// by mistake; only `add` may.
#[test]
fn test_list_and_remove_reject_a_missing_explicit_config() {
    let config = fresh_config("area_missing_config");
    assert!(err(&aoi(&config, &["list"], "")).contains("does not exist"));
    assert!(err(&aoi(&config, &["remove", "x", "--force"], "")).contains("does not exist"),);
    assert!(!config.exists());
}

/// Areas and ground stations share one file without disturbing each other.
#[test]
fn test_areas_and_ground_stations_coexist() {
    let config = fresh_config("area_with_stations");
    let gs = |args: &[&str], stdin: &str| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_sgp4-predict"))
            .args(["--config", config.to_str().unwrap(), "gs"])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to run sgp4-predict");
        let _ = child
            .stdin
            .take()
            .expect("stdin was piped")
            .write_all(stdin.as_bytes());
        child.wait_with_output().expect("failed to collect output")
    };

    ok(&gs(&["add"], "glasgow\n55.86\n-4.25\n40\n"));
    ok(&aoi(
        &config,
        &["add", "scotland", "--box", "57,-4.5,7,6"],
        "",
    ));

    assert!(ok(&gs(&["list"], "")).contains("glasgow"));
    assert!(ok(&aoi(&config, &["list"], "")).contains("scotland"));

    // Removing one leaves the other alone.
    ok(&aoi(&config, &["remove", "scotland", "--force"], ""));
    assert!(ok(&gs(&["list"], "")).contains("glasgow"));
}
