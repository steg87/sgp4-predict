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
    dir.join("nested").join("aois.yaml")
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
    let config = fresh_config("aoi_add_all_shapes");
    ok(&aoi(&config, &["add"], "scotland\nbox\n54\n60\n-8\n-1\n"));
    ok(&aoi(
        &config,
        &["add"],
        "north-sea\nellipse\n56\n2\n45\n2.7\n1.1\n",
    ));
    ok(&aoi(
        &config,
        &["add"],
        "cape-town\ncircle\n-33.9\n18.4\n2.25\n",
    ));
    ok(&aoi(
        &config,
        &["add"],
        "corridor\npolygon\n54,-8\n54,-1\n60,-1\n\n",
    ));

    let text = std::fs::read_to_string(&config).unwrap();
    for expected in [
        "shape: box",
        "south: 54.0",
        "east: -1.0",
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
    // Coordinates are stored one per named field, never as a list.
    assert!(!text.contains("54,60"), "{text}");
}

#[test]
fn test_add_creates_config_and_parents() {
    let config = fresh_config("aoi_add_creates");
    let out = aoi(&config, &["add"], "scotland\nbox\n54\n60\n-8\n-1\n");
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
    let config = fresh_config("aoi_list_fields");
    ok(&aoi(&config, &["add"], "scotland\nbox\n54\n60\n-8\n-1\n"));

    let listed = ok(&aoi(&config, &["list", "--format", "csv"], ""));
    assert!(
        listed.contains("scotland,box,south=54 north=60 west=-8 east=-1"),
        "{listed}"
    );
}

#[test]
fn test_ls_and_rm_aliases() {
    let config = fresh_config("aoi_aliases");
    ok(&aoi(&config, &["add"], "scotland\nbox\n54\n60\n-8\n-1\n"));
    assert!(ok(&aoi(&config, &["ls"], "")).contains("scotland"));

    ok(&aoi(&config, &["rm", "scotland", "--force"], ""));
    assert!(!ok(&aoi(&config, &["ls"], "")).contains("scotland"));
}

/// Anything but y/yes leaves the config alone, and so does EOF.
#[test]
fn test_remove_requires_confirmation() {
    let config = fresh_config("aoi_remove_confirm");
    ok(&aoi(&config, &["add"], "scotland\nbox\n54\n60\n-8\n-1\n"));

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
    let config = fresh_config("aoi_remove_unknown");
    ok(&aoi(&config, &["add"], "scotland\nbox\n54\n60\n-8\n-1\n"));

    let message = err(&aoi(&config, &["remove", "nowhere", "--force"], ""));
    assert!(message.contains("unknown aoi 'nowhere'"), "{message}");
    assert!(message.contains("known ids: scotland"), "{message}");
}

/// A duplicate id needs --force, so an existing AOI is never silently
/// replaced.
#[test]
fn test_add_refuses_to_overwrite_without_force() {
    let config = fresh_config("aoi_add_duplicate");
    ok(&aoi(&config, &["add"], "scotland\nbox\n54\n60\n-8\n-1\n"));

    let message = err(&aoi(&config, &["add", "scotland"], "circle\n0\n0\n1\n"));
    assert!(message.contains("already exists"), "{message}");
    assert!(
        ok(&aoi(&config, &["list"], "")).contains("box"),
        "unchanged"
    );

    ok(&aoi(
        &config,
        &["add", "scotland", "--force"],
        "circle\n0\n0\n1\n",
    ));
    assert!(ok(&aoi(&config, &["list"], "")).contains("circle"));
}

/// The shape may be named up front; only its coordinates are prompted for.
#[test]
fn test_shape_flag_skips_the_shape_prompt() {
    let config = fresh_config("aoi_shape_flag");
    let out = aoi(
        &config,
        &["add", "scotland", "--shape", "box"],
        "54\n60\n-8\n-1\n",
    );
    ok(&out);
    assert!(ok(&aoi(&config, &["list"], "")).contains("south=54 north=60"));
}

/// There is deliberately no flag carrying coordinates.
#[test]
fn test_no_coordinate_flags_exist() {
    let config = fresh_config("aoi_no_coord_flags");
    for flag in ["--box", "--ellipse", "--circle", "--poly"] {
        let message = err(&aoi(&config, &["add", "x", flag, "1,2,3,4"], ""));
        assert!(message.contains("unexpected argument"), "{flag}: {message}");
    }
}

#[test]
fn test_unknown_shape_flag_value_is_rejected() {
    let config = fresh_config("aoi_bad_shape_flag");
    let message = err(&aoi(&config, &["add", "x", "--shape", "hexagon"], ""));
    assert!(message.contains("invalid value 'hexagon'"), "{message}");
}

/// An id or shape given as an argument is echoed as though it had been typed,
/// so the transcript reads the same either way.
#[test]
fn test_arguments_are_echoed_like_prompts() {
    let config = fresh_config("aoi_echo");
    let out = aoi(
        &config,
        &["add", "scotland", "--shape", "box"],
        "54\n60\n-8\n-1\n",
    );
    ok(&out);

    let transcript = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(transcript.contains("AOI id: scotland"), "{transcript}");
    assert!(
        transcript.contains("Shape (box, ellipse, circle, polygon): box"),
        "{transcript}"
    );
}

/// With no id and no shape flag, `aoi add` prompts for everything, like
/// `gs add`.
#[test]
fn test_add_prompts_for_every_shape() {
    let config = fresh_config("aoi_prompt_shapes");
    ok(&aoi(&config, &["add"], "scotland\nbox\n54\n60\n-8\n-1\n"));
    ok(&aoi(
        &config,
        &["add"],
        "north-sea\nellipse\n56\n2\n45\n2.7\n1.1\n",
    ));
    ok(&aoi(
        &config,
        &["add"],
        "cape-town\ncircle\n-33.9\n18.4\n2.25\n",
    ));
    ok(&aoi(
        &config,
        &["add"],
        "corridor\npolygon\n54,-8\n54,-1\n60,-1\n\n",
    ));

    let listed = ok(&aoi(&config, &["list", "--format", "csv"], ""));
    assert!(listed.contains("scotland,box,south=54 north=60 west=-8 east=-1"));
    assert!(listed.contains(
        "north-sea,ellipse,latitude=56 longitude=2 semi_major=2.7 semi_minor=1.1 bearing=45"
    ));
    assert!(listed.contains("cape-town,circle,latitude=-33.9 longitude=18.4 radius=2.25"));
    // The definition contains commas, so CSV quotes it.
    assert!(listed.contains(r#"corridor,polygon,"(54, -8) (54, -1) (60, -1)""#));
}

/// Anything given as an argument consumes no input. The transcript looks the
/// same either way — that is the point of the echo — so this asserts on what
/// stdin is read for instead: each run supplies exactly the missing fields, and
/// a spurious re-prompt would swallow the next line and land the wrong values.
#[test]
fn test_arguments_consume_no_input() {
    let config = fresh_config("aoi_prompt_partial");

    // Id given: stdin starts at the shape.
    ok(&aoi(&config, &["add", "scotland"], "box\n54\n60\n-8\n-1\n"));
    // Shape given: stdin starts at the id.
    ok(&aoi(
        &config,
        &["add", "--shape", "box"],
        "north\n54\n60\n-8\n-1\n",
    ));
    // Both given: stdin holds coordinates alone.
    ok(&aoi(
        &config,
        &["add", "south", "--shape", "circle"],
        "10\n20\n3\n",
    ));

    let listed = ok(&aoi(&config, &["list", "--format", "csv"], ""));
    assert!(listed.contains("scotland,box,south=54 north=60 west=-8 east=-1"));
    assert!(listed.contains("north,box,south=54 north=60 west=-8 east=-1"));
    assert!(listed.contains("south,circle,latitude=10 longitude=20 radius=3"));
}

/// Vertices are numbered as they are entered, and a blank line ends the list.
#[test]
fn test_polygon_vertices_are_numbered_and_blank_line_ends_them() {
    let config = fresh_config("aoi_prompt_vertices");
    let out = aoi(
        &config,
        &["add"],
        "corridor\npolygon\n54,-8\n54,-1\n60,-1\n60,-8\n\n",
    );
    ok(&out);

    let prompts = String::from_utf8_lossy(&out.stderr).into_owned();
    for n in 1..=5 {
        assert!(
            prompts.contains(&format!("Vertex {n} lat,lon (degrees)")),
            "vertex {n} was not numbered:\n{prompts}"
        );
    }
    assert!(ok(&aoi(&config, &["list"], "")).contains("(60, -8)"));
}

/// A typo re-asks the same field instead of discarding everything before it.
#[test]
fn test_malformed_input_re_prompts_rather_than_aborting() {
    let config = fresh_config("aoi_prompt_retry");

    // A bad shape, then a bad number, then a good run of the same entry.
    let out = aoi(
        &config,
        &["add"],
        "scotland\nhexagon\nbox\n54\nsixty\n60\n-8\n-1\n",
    );
    ok(&out);
    let prompts = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(prompts.contains("unknown shape 'hexagon'"), "{prompts}");
    assert!(
        prompts.contains("expected a number, got 'sixty'"),
        "{prompts}"
    );
    assert!(
        ok(&aoi(&config, &["list"], "")).contains("south=54 north=60"),
        "the entry survived the typos"
    );
}

/// The bearing is asked before the axes, so each can be described relative to
/// it rather than leaving the reader to guess which is latitude.
#[test]
fn test_ellipse_prompts_bearing_before_the_axes() {
    let config = fresh_config("aoi_prompt_ellipse_order");
    let out = aoi(&config, &["add"], "north-sea\ne\n56\n2\n45\n2.7\n1.1\n");
    ok(&out);

    let prompts = String::from_utf8_lossy(&out.stderr).into_owned();
    let at = |needle: &str| {
        prompts
            .find(needle)
            .unwrap_or_else(|| panic!("missing {needle} in:\n{prompts}"))
    };
    assert!(at("Bearing of the long axis") < at("Semi-major axis"));
    assert!(at("Semi-major axis") < at("Semi-minor axis"));
    assert!(prompts.contains("ALONG that bearing"), "{prompts}");
    assert!(prompts.contains("ACROSS it"), "{prompts}");
}

/// A semi-minor axis larger than the semi-major is the likely confusion, so it
/// is caught at the prompt with the fix spelled out, not at the end.
#[test]
fn test_semi_minor_above_semi_major_re_prompts_with_the_fix() {
    let config = fresh_config("aoi_prompt_ellipse_swapped");
    // Wanted 10 east-west by 2 north-south, entered the axes the wrong way round.
    let out = aoi(&config, &["add"], "wide\ne\n0\n0\n90\n2\n10\n1\n");
    ok(&out);

    let prompts = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        prompts.contains("cannot exceed the semi-major axis of 2"),
        "{prompts}"
    );
    assert!(
        prompts.contains("turn the bearing by 90 degrees"),
        "{prompts}"
    );
    // The centre and bearing entered before the mistake survived it.
    assert!(ok(&aoi(&config, &["list"], "")).contains("bearing=90"));
}

/// A malformed vertex re-asks at the same index, so earlier ones are kept.
#[test]
fn test_malformed_vertex_re_prompts_at_the_same_index() {
    let config = fresh_config("aoi_prompt_vertex_retry");
    let out = aoi(
        &config,
        &["add"],
        "corridor\npolygon\n54,-8\nnorth-west\n54,-1\n60,-1\n\n",
    );
    ok(&out);

    let prompts = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(prompts.contains("expected `lat,lon`"), "{prompts}");
    // Three good vertices were kept despite the bad line between them.
    assert!(ok(&aoi(&config, &["list"], "")).contains("(54, -8) (54, -1) (60, -1)"));
}

/// A blank line before the third vertex re-asks rather than throwing away the
/// vertices already entered.
#[test]
fn test_blank_line_too_early_keeps_the_vertices_so_far() {
    let config = fresh_config("aoi_prompt_vertex_early_blank");
    let out = aoi(
        &config,
        &["add"],
        "corridor\npolygon\n54,-8\n54,-1\n\n60,-1\n\n",
    );
    ok(&out);

    let prompts = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        prompts.contains("needs at least 3 vertices; 2 so far"),
        "{prompts}"
    );
    assert!(ok(&aoi(&config, &["list"], "")).contains("(54, -8) (54, -1) (60, -1)"));
}

/// Prompting still ends at end-of-input, so a scripted caller cannot spin.
#[test]
fn test_end_of_input_ends_prompting() {
    let config = fresh_config("aoi_prompt_eof");
    let message = err(&aoi(&config, &["add"], "scotland\n"));
    assert!(message.contains("unexpected end of input"), "{message}");
    assert!(!config.exists(), "nothing should have been written");
}

/// Geometry the library rejects never reaches the file.
///
/// A polygon is the case the prompts cannot pre-empt: each vertex is
/// individually fine, and only the assembled ring is too big for a hemisphere.
#[test]
fn test_invalid_geometry_is_rejected_before_saving() {
    let config = fresh_config("aoi_invalid_geometry");
    let message = err(&aoi(
        &config,
        &["add", "x"],
        "polygon\n0,0\n10,130\n-10,-130\n\n",
    ));
    assert!(message.contains("hemisphere"), "{message}");
    assert!(
        !config.exists(),
        "a rejected aoi must not create the config"
    );
}

/// Out-of-range extents are caught at their own prompt, naming the field,
/// rather than surfacing later as a confusing corner error.
#[test]
fn test_out_of_range_extents_re_prompt() {
    let config = fresh_config("aoi_extent_range");

    let out = aoi(&config, &["add"], "big\ncircle\n0\n0\n100\n2\n");
    ok(&out);
    let prompts = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        prompts.contains("less than 90 degrees, got 100"),
        "{prompts}"
    );
}

/// Each box bound is checked against the field it was typed into. Nothing is
/// derived, so the message can name the bound that was wrong.
#[test]
fn test_box_bounds_are_checked_at_their_own_prompt() {
    let config = fresh_config("aoi_box_bounds");

    // Latitude past a pole, a north bound below the south one, and an east
    // bound on the same meridian as the west one — each re-asks in place.
    let out = aoi(
        &config,
        &["add"],
        "scotland\nbox\n95\n54\n40\n60\n-8\n-8\n-1\n",
    );
    ok(&out);
    let prompts = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        prompts.contains("latitude must be between -90 and 90 degrees, got 95"),
        "{prompts}"
    );
    assert!(
        prompts.contains("must lie north of the south bound of 54"),
        "{prompts}"
    );
    assert!(
        prompts.contains("same meridian as the west bound of -8"),
        "{prompts}"
    );
    // Everything entered before each mistake survived it.
    assert!(ok(&aoi(&config, &["list"], "")).contains("south=54 north=60 west=-8 east=-1"));
}

/// A box whose east bound is west of its west bound wraps the antimeridian
/// rather than being read as an error.
#[test]
fn test_box_may_wrap_the_antimeridian() {
    let config = fresh_config("aoi_box_wrap");
    ok(&aoi(
        &config,
        &["add"],
        "pacific\nbox\n-20\n20\n160\n-160\n",
    ));
    assert!(ok(&aoi(&config, &["list"], "")).contains("west=160 east=-160"));
}

#[test]
fn test_negative_coordinates_are_accepted() {
    let config = fresh_config("aoi_negative");
    ok(&aoi(&config, &["add"], "cape\ncircle\n-33.9\n18.4\n2\n"));
    assert!(ok(&aoi(&config, &["list"], "")).contains("latitude=-33.9"));
}

/// `aoi list` and `aoi remove` must not create a config the user pointed at
/// by mistake; only `add` may.
#[test]
fn test_list_and_remove_reject_a_missing_explicit_config() {
    let config = fresh_config("aoi_missing_config");
    assert!(err(&aoi(&config, &["list"], "")).contains("does not exist"));
    assert!(err(&aoi(&config, &["remove", "x", "--force"], "")).contains("does not exist"),);
    assert!(!config.exists());
}

/// AOIs and ground stations share one file without disturbing each other.
#[test]
fn test_aois_and_ground_stations_coexist() {
    let config = fresh_config("aoi_with_stations");
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
    ok(&aoi(&config, &["add"], "scotland\nbox\n54\n60\n-8\n-1\n"));

    assert!(ok(&gs(&["list"], "")).contains("glasgow"));
    assert!(ok(&aoi(&config, &["list"], "")).contains("scotland"));

    // Removing one leaves the other alone.
    ok(&aoi(&config, &["remove", "scotland", "--force"], ""));
    assert!(ok(&gs(&["list"], "")).contains("glasgow"));
}
