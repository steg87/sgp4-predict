//! `sgp4-predict aoi add|remove|list` — manage the config file's areas of interest.
//!
//! `aoi add` prompts field by field like `gs add`. The id and the shape may be
//! given on the command line, but the coordinates never are — hand-writing an
//! AOI is what editing the config file is for. A polygon has no fixed field
//! count, so its vertices are read in a numbered loop that a blank line ends.
//! Prompts and confirmations go to stderr, so `aoi list` stays pipeable.

use std::{
    io::{BufRead as _, IsTerminal as _},
    path::Path,
};

use super::{confirm, echo, prompt, prompt_f64, prompt_retry};
use crate::{
    aoi::AoiShape,
    cli::{AoiAddArgs, AoiCommand, AoiListArgs, AoiRemoveArgs, Shape},
    config::{self, AoiDef, BoxDef, CircleDef, Config, EllipseDef, PolygonDef, Vertex},
    output,
};

/// The stdin line iterator the prompts read from.
type Lines<'a> = std::io::Lines<std::io::StdinLock<'a>>;

pub fn run(command: AoiCommand, config_path: Option<&Path>) -> anyhow::Result<()> {
    // Only `add` may bring a config file into existence.
    let missing = match command {
        AoiCommand::Add(_) => config::Missing::Create,
        AoiCommand::Remove(_) | AoiCommand::List(_) => config::Missing::Reject,
    };
    let (config, path) = config::open_for_edit(config_path, missing)?;

    match command {
        AoiCommand::Add(args) => add(config, &path, args),
        AoiCommand::Remove(args) => remove(config, &path, args),
        AoiCommand::List(args) => list(&config, &path, args),
    }
}

fn add(mut config: Config, path: &Path, args: AoiAddArgs) -> anyhow::Result<()> {
    let existed = path.is_file();
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();

    let id = match args.id {
        Some(id) => {
            echo("AOI id", &id);
            id
        }
        None => prompt(&mut lines, "AOI id")?,
    };
    // Checked before the shape, so a clashing id is not discovered only after
    // every coordinate has been typed in.
    anyhow::ensure!(!id.is_empty(), "aoi id cannot be empty");
    anyhow::ensure!(
        args.force || !config.aois.contains_key(&id),
        "aoi '{id}' already exists; pass --force to replace it, or pick another id"
    );

    let shape_kind = match args.shape {
        Some(shape) => {
            echo(&shape_label(), crate::cli::value_name(shape));
            shape
        }
        None => prompt_shape(&mut lines)?,
    };
    let def = prompt_definition(&mut lines, shape_kind)?;
    // Reject a shape the library cannot build before it reaches the file,
    // rather than on the next lookup.
    let shape = def.build()?;

    config.aois.insert(id.clone(), def);
    config.save(path)?;

    if !existed {
        eprintln!("created {}", path.display());
    }
    eprintln!(
        "added aoi '{id}' ({}) to {}",
        summarise(&shape),
        path.display()
    );
    Ok(())
}

/// Ask which shape it is.
///
/// Each name's initial is accepted on its own, and is underlined in the prompt
/// when stderr is a terminal.
fn prompt_shape(lines: &mut Lines) -> anyhow::Result<Shape> {
    prompt_retry(lines, &shape_label(), |input| {
        match input.to_ascii_lowercase().as_str() {
            "b" | "box" => Ok(Shape::Box),
            "e" | "ellipse" => Ok(Shape::Ellipse),
            "c" | "circle" => Ok(Shape::Circle),
            "p" | "poly" | "polygon" => Ok(Shape::Polygon),
            other => anyhow::bail!(
                "unknown shape '{other}'; expected box, ellipse, circle or polygon (or b/e/c/p)"
            ),
        }
    })
}

/// Ask for the shape's coordinates. Always prompted — there is no flag
/// carrying them, so ranges are checked here rather than in a value parser.
fn prompt_definition(lines: &mut Lines, shape: Shape) -> anyhow::Result<AoiDef> {
    Ok(match shape {
        Shape::Box => prompt_box(lines)?,
        Shape::Ellipse => prompt_ellipse(lines)?,
        Shape::Circle => AoiDef::Circle(CircleDef {
            latitude: prompt_latitude(lines, "Centre latitude (degrees)")?,
            longitude: prompt_f64(lines, "Centre longitude (degrees)", None)?,
            radius: prompt_bounded(lines, "Radius (degrees)", 90.0)?,
        }),
        Shape::Polygon => AoiDef::Polygon(PolygonDef {
            vertices: prompt_vertices(lines)?,
        }),
    })
}

/// The shape prompt's label, also used when echoing a `--shape` flag.
fn shape_label() -> String {
    format!(
        "Shape ({}, {}, {}, {})",
        initial("box"),
        initial("ellipse"),
        initial("circle"),
        initial("polygon"),
    )
}

/// Prompt for an extent in `(0, limit)`.
fn prompt_bounded(lines: &mut Lines, label: &str, limit: f64) -> anyhow::Result<f64> {
    prompt_retry(lines, label, move |input| {
        let value = number(input)?;
        anyhow::ensure!(
            value > 0.0 && value < limit,
            "must be greater than 0 and less than {limit} degrees, got {value}"
        );
        Ok(value)
    })
}

/// Prompt for a latitude in `[-90, 90]`.
fn prompt_latitude(lines: &mut Lines, label: &str) -> anyhow::Result<f64> {
    prompt_retry(lines, label, latitude)
}

fn latitude(input: &str) -> anyhow::Result<f64> {
    let value = number(input)?;
    anyhow::ensure!(
        (-90.0..=90.0).contains(&value),
        "latitude must be between -90 and 90 degrees, got {value}"
    );
    Ok(value)
}

/// Ask for a box, as the four bounds the library itself takes.
///
/// Each bound is checked against the field it was typed into: unlike a centre
/// with extents, there is nothing derived here, so "which value was wrong" is
/// never ambiguous.
fn prompt_box(lines: &mut Lines) -> anyhow::Result<AoiDef> {
    let south = prompt_latitude(lines, "South latitude (degrees)")?;
    let north = prompt_retry(lines, "North latitude (degrees)", |input| {
        let value = latitude(input)?;
        anyhow::ensure!(
            value > south,
            "must lie north of the south bound of {south}"
        );
        Ok(value)
    })?;

    let west = prompt_f64(lines, "West longitude (degrees)", None)?;
    // The box runs eastward from `west`, so two bounds on the same meridian
    // are a box with no width rather than one wrapping the whole world.
    let east = prompt_retry(lines, "East longitude (degrees)", |input| {
        let value = number(input)?;
        anyhow::ensure!(
            (value - west).rem_euclid(360.0) > 1e-7,
            "is the same meridian as the west bound of {west}, so the box has no width"
        );
        Ok(value)
    })?;

    Ok(AoiDef::Box(BoxDef {
        south,
        north,
        west,
        east,
    }))
}

/// Ask for an ellipse, **bearing first**.
///
/// The semi-axes are not latitude and longitude extents — semi-major is simply
/// the longer one, and the bearing is what points it. Asking for the bearing
/// first means the two axes can be described as along and across it, rather
/// than leaving the reader to work out which is which.
fn prompt_ellipse(lines: &mut Lines) -> anyhow::Result<AoiDef> {
    let latitude = prompt_latitude(lines, "Centre latitude (degrees)")?;
    let longitude = prompt_f64(lines, "Centre longitude (degrees)", None)?;
    let bearing = prompt_f64(
        lines,
        "Bearing of the long axis, degrees clockwise from north (0 = north-south)",
        Some(0.0),
    )?;

    let semi_major = prompt_retry(
        lines,
        "Semi-major axis, half the length ALONG that bearing (degrees)",
        |input| {
            let value = number(input)?;
            anyhow::ensure!(
                value > 0.0 && value < 90.0,
                "must be greater than 0 and less than 90 degrees, got {value}"
            );
            Ok(value)
        },
    )?;
    // Checked here, not at `build()`, so the fix is offered while the value is
    // still on screen instead of after every field has been entered.
    let semi_minor = prompt_retry(
        lines,
        "Semi-minor axis, half the width ACROSS it (degrees)",
        |input| {
            let value = number(input)?;
            anyhow::ensure!(value > 0.0, "must be greater than 0, got {value}");
            anyhow::ensure!(
                value <= semi_major,
                "cannot exceed the semi-major axis of {semi_major}; for a wider-than-long \
                 AOI, swap the two and turn the bearing by 90 degrees"
            );
            Ok(value)
        },
    )?;

    Ok(AoiDef::Ellipse(EllipseDef {
        latitude,
        longitude,
        semi_major,
        semi_minor,
        bearing,
    }))
}

/// `nan`/`inf` parse as `f64` but pass every range check silently.
fn number(input: &str) -> anyhow::Result<f64> {
    match input.parse::<f64>() {
        Ok(value) if value.is_finite() => Ok(value),
        _ => anyhow::bail!("expected a number, got '{input}'"),
    }
}

/// Read vertices until a blank line. Numbered as they are entered, since a
/// polygon is the one shape with no fixed field count to orient by.
///
/// A malformed line and a too-early blank line both re-ask at the same index,
/// so nothing already entered is lost.
fn prompt_vertices(lines: &mut Lines) -> anyhow::Result<Vec<Vertex>> {
    const MIN_VERTICES: usize = 3;
    eprintln!("Vertices, one per line as `lat,lon`. Blank line when done.");

    let mut vertices = Vec::new();
    loop {
        let label = format!("Vertex {} lat,lon (degrees)", vertices.len() + 1);
        let input = prompt(lines, &label)?;
        if input.is_empty() {
            if vertices.len() >= MIN_VERTICES {
                return Ok(vertices);
            }
            eprintln!(
                "  a polygon needs at least {MIN_VERTICES} vertices; {} so far",
                vertices.len()
            );
            continue;
        }
        match parse_vertex(&input) {
            Ok(vertex) => vertices.push(vertex),
            Err(e) => eprintln!("  {e:#}"),
        }
    }
}

/// A shape name with its shorthand initial underlined.
///
/// Only when stderr is a terminal — a redirected prompt would otherwise carry
/// escape codes into a log, and the tests read stderr as plain text.
fn initial(word: &str) -> String {
    let mut chars = word.chars();
    match (chars.next(), std::io::stderr().is_terminal()) {
        (Some(first), true) => format!("\x1b[4m{first}\x1b[24m{}", chars.as_str()),
        _ => word.to_string(),
    }
}

fn parse_vertex(input: &str) -> anyhow::Result<Vertex> {
    let (lat, lon) = input
        .split_once(',')
        .ok_or_else(|| anyhow::anyhow!("expected `lat,lon`, got '{input}'"))?;
    Ok(Vertex {
        latitude: latitude(lat.trim())?,
        longitude: number(lon.trim())?,
    })
}

fn remove(mut config: Config, path: &Path, args: AoiRemoveArgs) -> anyhow::Result<()> {
    // find_aoi(), not a build: removal must not require a valid shape, or a
    // hand-edited bad entry could only be deleted by editing the YAML.
    let def = config.find_aoi(&args.id)?;

    if !args.force {
        eprintln!("{}: {} {}", args.id, def.kind(), def.describe());
        if !confirm(&format!("Remove aoi '{}'?", args.id))? {
            eprintln!("aborted; nothing was changed");
            return Ok(());
        }
    }

    config.aois.remove(&args.id);
    config.save(path)?;

    eprintln!("removed aoi '{}' from {}", args.id, path.display());
    Ok(())
}

fn list(config: &Config, path: &Path, args: AoiListArgs) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    output::write_aois(
        stdout.lock(),
        args.format,
        config.aois.iter().map(|(id, def)| (id.as_str(), def)),
    )?;

    // An empty table on a machine with no config looks like "you have no
    // AOIs" when the file simply is not there yet. Say which it is.
    if config.aois.is_empty() && !path.is_file() {
        eprintln!(
            "no config at {}; add an aoi with `sgp4-predict aoi add`",
            path.display()
        );
    }
    Ok(())
}

/// The built shape's extent, for the confirmation line — derived from the
/// library types, so it reports what was actually constructed rather than what
/// was typed.
fn summarise(shape: &AoiShape) -> String {
    // Bounds round-trip through radians, so trim the float noise rather than
    // reporting a box as "54..59.99999999999999".
    fn deg(value: sgp4_predict::Degrees) -> f64 {
        (value.to_f64() * 1e6).round() / 1e6
    }
    match shape {
        AoiShape::Rectangle(r) => {
            let (south, north) = r.latitudes();
            let (west, span) = r.longitudes();
            format!(
                "{}..{}, {} eastward from {}",
                deg(south),
                deg(north),
                deg(span),
                deg(west)
            )
        }
        AoiShape::Ellipse(e) => {
            let centre = e.centre();
            let (a, b) = e.semi_axes();
            format!(
                "centred {},{}, {} by {} at bearing {}",
                deg(centre.latitude),
                deg(centre.longitude),
                deg(a),
                deg(b),
                deg(e.bearing())
            )
        }
        AoiShape::Polygon(p) => format!("{} vertices", p.vertices().len()),
    }
}
