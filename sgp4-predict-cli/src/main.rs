mod cli;
mod commands;
mod observer;
mod output;
mod tle;

use clap::Parser as _;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();

    let args = cli::Args::parse();
    match args.command {
        cli::Command::Observations(obs_args) => commands::observations::run(obs_args),
        cli::Command::Transits(transit_args) => commands::transits::run(transit_args),
        cli::Command::StateVectors(sv_args) => commands::state_vectors::run(sv_args),
        cli::Command::Apsides(args) => commands::apsides::run(args),
        cli::Command::Illumination(args) => commands::illumination::run(args),
    }
}
