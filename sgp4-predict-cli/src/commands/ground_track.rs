use anyhow::Context as _;
use chrono::Duration;

use super::prepare;
use crate::{cli::GroundTrackArgs, output};

pub fn run(args: GroundTrackArgs) -> anyhow::Result<()> {
    let step = Duration::from_std(args.step).context("step out of range")?;
    let mut ctx = prepare(&args.common)?;

    let step_str = humantime::format_duration(args.step).to_string();
    ctx.write_args_header("ground-track", &args.common, None, &[("step", &step_str)])?;

    let rows = ctx.predictor.ground_track_iter(ctx.interval.clone(), step);
    output::write_ground_track(ctx.writer, ctx.format, rows)
}
