//! `sgp4-predict aoi add|remove|list` — manage the config file's areas of interest.
//!
//! Unlike `gs add`, `aoi add` takes its shape as a flag rather than prompting:
//! a polygon is a list of arbitrary length, which does not survive a
//! field-at-a-time prompt. Confirmations still go to stderr, so `aoi list`
//! stays pipeable.

use std::path::Path;

use super::confirm;
use crate::{
    area::AreaShape,
    cli::{AoiAddArgs, AoiCommand, AoiListArgs, AoiRemoveArgs},
    config::{self, Config},
    output,
};

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
    anyhow::ensure!(!args.id.is_empty(), "area id cannot be empty");
    anyhow::ensure!(
        args.force || !config.areas.contains_key(&args.id),
        "area '{}' already exists; pass --force to replace it, or pick another id",
        args.id
    );

    let def = args.shape.resolve();
    // Reject a shape the library cannot build before it reaches the file,
    // rather than on the next lookup.
    let shape = def.build()?;

    config.areas.insert(args.id.clone(), def);
    config.save(path)?;

    if !existed {
        eprintln!("created {}", path.display());
    }
    eprintln!(
        "added area '{}' ({}) to {}",
        args.id,
        summarise(&shape),
        path.display()
    );
    Ok(())
}

fn remove(mut config: Config, path: &Path, args: AoiRemoveArgs) -> anyhow::Result<()> {
    // find_area(), not a build: removal must not require a valid shape, or a
    // hand-edited bad entry could only be deleted by editing the YAML.
    let def = config.find_area(&args.id)?;

    if !args.force {
        eprintln!("{}: {} {}", args.id, def.kind(), def.describe());
        if !confirm(&format!("Remove area '{}'?", args.id))? {
            eprintln!("aborted; nothing was changed");
            return Ok(());
        }
    }

    config.areas.remove(&args.id);
    config.save(path)?;

    eprintln!("removed area '{}' from {}", args.id, path.display());
    Ok(())
}

fn list(config: &Config, path: &Path, args: AoiListArgs) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    output::write_areas(
        stdout.lock(),
        args.format,
        config.areas.iter().map(|(id, def)| (id.as_str(), def)),
    )?;

    // An empty table on a machine with no config looks like "you have no
    // areas" when the file simply is not there yet. Say which it is.
    if config.areas.is_empty() && !path.is_file() {
        eprintln!(
            "no config at {}; add an area with `sgp4-predict aoi add`",
            path.display()
        );
    }
    Ok(())
}

/// The built shape's extent, for the confirmation line — derived from the
/// library types, so it reports what was actually constructed rather than what
/// was typed.
fn summarise(shape: &AreaShape) -> String {
    // Bounds round-trip through radians, so trim the float noise rather than
    // reporting a box as "54..59.99999999999999".
    fn deg(value: sgp4_predict::Degrees) -> f64 {
        (value.to_f64() * 1e6).round() / 1e6
    }
    match shape {
        AreaShape::Rectangle(r) => {
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
        AreaShape::Ellipse(e) => {
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
        AreaShape::Polygon(p) => format!("{} vertices", p.vertices().len()),
    }
}
