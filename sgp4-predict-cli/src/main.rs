mod cli;
mod commands;
mod config;
mod observer;
mod output;
mod tle;

use clap::Parser as _;
use std::io::ErrorKind;

/// Exit code for a command terminated by a closed pipe, per shell convention
/// (128 + SIGPIPE).
const EXIT_SIGPIPE: i32 = 141;

fn main() -> std::process::ExitCode {
    let args = cli::Args::parse();
    init_tracing(&args);

    match run(args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        // `cmd | head` closes the pipe early; that is normal use, not a failure
        // worth printing an error for.
        Err(e) if is_broken_pipe(&e) => std::process::ExitCode::from(EXIT_SIGPIPE as u8),
        Err(e) => {
            eprintln!("Error: {e:?}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(args: cli::Args) -> anyhow::Result<()> {
    let config_path = args.config.as_deref();
    match args.command {
        cli::Command::Observations(a) => commands::observations::run(a, config_path),
        cli::Command::Transits(a) => commands::transits::run(a, config_path),
        cli::Command::StateVectors(a) => commands::state_vectors::run(a),
        cli::Command::Apsides(a) => commands::apsides::run(a),
        cli::Command::Illumination(a) => commands::illumination::run(a),
    }
}

/// `RUST_LOG` wins if set; otherwise `--verbose` / `--quiet` choose the level.
fn init_tracing(args: &cli::Args) {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| args.log_level().into());
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .without_time()
        .with_target(false)
        .with_env_filter(filter)
        .init();
}

/// True if any error in the chain is a broken pipe.
fn is_broken_pipe(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == ErrorKind::BrokenPipe)
    })
}
