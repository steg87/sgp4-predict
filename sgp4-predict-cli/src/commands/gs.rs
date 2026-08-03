//! `sgp4-predict gs add|remove|list` — manage the config file's ground stations.

use std::{io::BufRead as _, path::Path};

use super::{confirm, prompt, prompt_f64};
use crate::{
    cli::{GsCommand, GsListArgs, GsRemoveArgs},
    config::{self, Config, GroundStation, Location},
    output,
};

pub fn run(command: GsCommand, config_path: Option<&Path>) -> anyhow::Result<()> {
    // Only `add` may bring a config file into existence.
    let missing = match command {
        GsCommand::Add => config::Missing::Create,
        GsCommand::Remove(_) | GsCommand::List(_) => config::Missing::Reject,
    };
    let (config, path) = config::open_for_edit(config_path, missing)?;

    match command {
        GsCommand::Add => add(config, &path),
        GsCommand::Remove(args) => remove(config, &path, args),
        GsCommand::List(args) => list(&config, &path, args),
    }
}

fn add(mut config: Config, path: &Path) -> anyhow::Result<()> {
    let existed = path.is_file();
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();

    let id = prompt(&mut lines, "Ground station id")?;
    anyhow::ensure!(!id.is_empty(), "ground station id cannot be empty");
    anyhow::ensure!(
        !config.groundstations.contains_key(&id),
        "ground station '{id}' already exists; remove it first or pick another id"
    );

    let location = Location {
        latitude: prompt_f64(&mut lines, "Latitude (degrees)", None)?,
        longitude: prompt_f64(&mut lines, "Longitude (degrees)", None)?,
        altitude: prompt_f64(&mut lines, "Altitude (metres)", Some(0.0))?,
    };
    // Reject a bad location before it reaches the file, not on next lookup.
    location.validate()?;

    config
        .groundstations
        .insert(id.clone(), GroundStation { location });
    config.save(path)?;

    if !existed {
        eprintln!("created {}", path.display());
    }
    eprintln!("added ground station '{id}' to {}", path.display());
    Ok(())
}

fn remove(mut config: Config, path: &Path, args: GsRemoveArgs) -> anyhow::Result<()> {
    // find(), not groundstation(): removal must not require valid coordinates,
    // or a hand-edited bad entry could only be deleted by editing the YAML.
    let station = config.find(&args.id)?;

    if !args.force {
        eprintln!("{}", describe(&args.id, station));
        if !confirm(&format!("Remove ground station '{}'?", args.id))? {
            eprintln!("aborted; nothing was changed");
            return Ok(());
        }
    }

    config.groundstations.remove(&args.id);
    config.save(path)?;

    eprintln!(
        "removed ground station '{}' from {}",
        args.id,
        path.display()
    );
    Ok(())
}

fn list(config: &Config, path: &Path, args: GsListArgs) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    output::write_ground_stations(
        stdout.lock(),
        args.format,
        config
            .groundstations
            .iter()
            .map(|(id, station)| (id.as_str(), station)),
    )?;

    // An empty table on a machine with no config looks like "you have no
    // stations" when the file simply is not there yet. Say which it is.
    if config.groundstations.is_empty() && !path.is_file() {
        eprintln!(
            "no config at {}; add a ground station with `sgp4-predict gs add`",
            path.display()
        );
    }
    Ok(())
}

fn describe(id: &str, station: &GroundStation) -> String {
    format!(
        "{id}: latitude {}, longitude {}, altitude {} m",
        station.location.latitude, station.location.longitude, station.location.altitude
    )
}
