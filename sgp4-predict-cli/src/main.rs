mod aoi;
mod cli;
mod commands;
mod config;
mod observer;
mod output;
mod tle;

use anyhow::Context as _;
use clap::{CommandFactory as _, Parser as _};
use std::io::{ErrorKind, Write as _};

/// Exit code for a command terminated by a closed pipe, per shell convention
/// (128 + SIGPIPE).
const EXIT_SIGPIPE: i32 = 141;

fn main() -> std::process::ExitCode {
    let args = cli::Args::parse();
    init_tracing(&args);

    match run(args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        // `cmd | head` closes the pipe early; that is normal use, not a failure
        // worth printing a backtrace-style error for.
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
        cli::Command::GroundTrack(a) => commands::ground_track::run(a),
        cli::Command::AoiWindows(a) => commands::aoi_windows::run(a, config_path),
        cli::Command::Gs(a) => commands::gs::run(a.command, config_path),
        cli::Command::Aoi(a) => commands::aoi::run(a.command, config_path),
        cli::Command::Completions(a) => {
            // clap_complete panics on write errors instead of returning them,
            // so render into memory and do the writing here.
            let mut script = Vec::new();
            clap_complete::generate(
                a.shell,
                &mut cli::Args::command(),
                "sgp4-predict",
                &mut script,
            );
            std::io::stdout()
                .write_all(&script)
                .context("failed to write completions")
        }
        // Generated from the live clap Command rather than a build script, so
        // it cannot drift from the actual flags.
        cli::Command::Man => clap_mangen::Man::new(cli::Args::command())
            .render(&mut std::io::stdout())
            .context("failed to render man page"),
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
