//! `sgp4-predict gs add|remove|list` — manage the config file's ground stations.

use anyhow::Context as _;
use std::{
    io::{BufRead as _, Write as _},
    path::Path,
};

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
        GsCommand::List(args) => list(&config, args),
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
    // Report an unknown id with the usual "known ids" hint before prompting.
    let station = config.groundstation(&args.id)?;

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

fn list(config: &Config, args: GsListArgs) -> anyhow::Result<()> {
    let stdout = std::io::stdout();
    output::write_ground_stations(
        stdout.lock(),
        args.format,
        config
            .groundstations
            .iter()
            .map(|(id, station)| (id.as_str(), station)),
    )
}

fn describe(id: &str, station: &GroundStation) -> String {
    format!(
        "{id}: latitude {}, longitude {}, altitude {} m",
        station.location.latitude, station.location.longitude, station.location.altitude
    )
}

/// Prompts go to stderr so `gs list`-style piping is never contaminated and
/// the prompts stay visible when stdout is redirected.
fn prompt(
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
    label: &str,
) -> anyhow::Result<String> {
    eprint!("{label}: ");
    std::io::stderr().flush()?;
    let line = lines
        .next()
        .ok_or_else(|| anyhow::anyhow!("unexpected end of input while reading {label}"))?
        .context("failed to read from stdin")?;
    Ok(line.trim().to_string())
}

fn prompt_f64(
    lines: &mut impl Iterator<Item = std::io::Result<String>>,
    label: &str,
    default: Option<f64>,
) -> anyhow::Result<f64> {
    let shown = match default {
        Some(d) => format!("{label} [{d}]"),
        None => label.to_string(),
    };
    let input = prompt(lines, &shown)?;
    match (input.as_str(), default) {
        ("", Some(d)) => Ok(d),
        ("", None) => anyhow::bail!("{label} is required"),
        (value, _) => value
            .parse()
            .map_err(|_| anyhow::anyhow!("{label}: expected a number, got '{value}'")),
    }
}

/// Ask a yes/no question. Anything other than y/yes means no, and so does EOF,
/// so a non-interactive caller that forgot `--force` cannot delete anything.
fn confirm(question: &str) -> anyhow::Result<bool> {
    eprint!("{question} [y/N] ");
    std::io::stderr().flush()?;

    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer)? == 0 {
        return Ok(false);
    }
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}
