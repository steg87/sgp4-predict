use anyhow::Context as _;
use chrono::Duration;
use std::path::Path;

use super::{format_observer_str, prepare};
use crate::{cli::ObservationsArgs, output};

pub fn run(args: ObservationsArgs, config_path: Option<&Path>) -> anyhow::Result<()> {
    // Resolve the station first: an unknown --gs must fail before the user is
    // asked for a TLE on stdin.
    let observer = args.observer.resolve(config_path)?;
    let step = Duration::from_std(args.step).context("step out of range")?;
    let mut ctx = prepare(&args.common)?;

    let observer_str = format_observer_str(&observer);
    let gs_id = args.observer.gs.as_deref().expect("resolve requires --gs");
    let step_str = humantime::format_duration(args.step).to_string();
    ctx.write_args_header(
        "observations",
        &args.common,
        &[
            ("ground-station", gs_id),
            ("observer", &observer_str),
            ("step", &step_str),
        ],
    )?;

    let observations = ctx
        .predictor
        .observation_iter(&observer, ctx.interval.clone(), step);
    output::write_observations(ctx.writer, ctx.format, observations)
}
