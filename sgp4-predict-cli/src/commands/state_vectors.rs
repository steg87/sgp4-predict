use anyhow::Context as _;
use chrono::Duration;
use sgp4_predict::Predictor;

use super::{load_sat, open_writer, resolve_interval, warn_stale_tle};
use crate::{
    cli::{Frame, StateVectorsArgs},
    output,
};

pub fn run(args: StateVectorsArgs) -> anyhow::Result<()> {
    let (start, interval) = resolve_interval(&args.common)?;
    let step = Duration::from_std(args.step).context("step out of range")?;
    let sat = load_sat(&args.common)?;
    let predictor = Predictor::new(&sat)?;
    warn_stale_tle(&predictor, start);
    let writer = open_writer(&args.common.out)?;

    match args.frame {
        Frame::Teme => output::write_state_vectors(
            writer,
            predictor.prediction_iter(interval, step).map(|r| {
                r.map(|(t, sv)| {
                    (
                        t,
                        sv.position.x,
                        sv.position.y,
                        sv.position.z,
                        sv.velocity.x,
                        sv.velocity.y,
                        sv.velocity.z,
                    )
                })
            }),
        ),
        Frame::Ecef => output::write_state_vectors(
            writer,
            predictor.prediction_iter(interval, step).map(|r| {
                r.map(|(t, sv)| {
                    let ecef = sv.to_ecef(t);
                    (
                        t,
                        ecef.position.x,
                        ecef.position.y,
                        ecef.position.z,
                        ecef.velocity.x,
                        ecef.velocity.y,
                        ecef.velocity.z,
                    )
                })
            }),
        ),
    }
}
