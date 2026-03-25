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
    }
}
