use anyhow::Context as _;
use chrono::Duration;

use super::prepare;
use crate::{
    cli::{Frame, StateVectorsArgs, value_name},
    output,
};

pub fn run(args: StateVectorsArgs) -> anyhow::Result<()> {
    let step = Duration::from_std(args.step).context("step out of range")?;
    let mut ctx = prepare(&args.common)?;

    let step_str = humantime::format_duration(args.step).to_string();
    let frame_str = value_name(args.frame);
    ctx.write_args_header(
        "state-vectors",
        &args.common,
        &[("step", &step_str), ("frame", &frame_str)],
    )?;

    let rows = ctx
        .predictor
        .prediction_iter(ctx.interval.clone(), step)
        .map(move |r| {
            r.map(|(t, sv)| {
                // StateVector<Teme> and StateVector<Ecef> are distinct types,
                // so the arms have to unify on the raw scalars.
                let (px, py, pz, vx, vy, vz) = match args.frame {
                    Frame::Teme => (
                        sv.position.x,
                        sv.position.y,
                        sv.position.z,
                        sv.velocity.x,
                        sv.velocity.y,
                        sv.velocity.z,
                    ),
                    Frame::Ecef => {
                        let e = sv.to_ecef(t);
                        (
                            e.position.x,
                            e.position.y,
                            e.position.z,
                            e.velocity.x,
                            e.velocity.y,
                            e.velocity.z,
                        )
                    }
                };
                (t, px, py, pz, vx, vy, vz)
            })
        });

    output::write_state_vectors(ctx.writer, ctx.format, rows)
}
