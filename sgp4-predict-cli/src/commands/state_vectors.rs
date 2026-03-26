use anyhow::Context as _;
use chrono::Duration;
use sgp4_predict::Predictor;

use super::{load_tle, open_writer, resolve_interval, warn_stale_tle, write_args_header};
use crate::{
    cli::{Frame, StateVectorsArgs},
    output,
};

pub fn run(args: StateVectorsArgs) -> anyhow::Result<()> {
    let (start, interval) = resolve_interval(&args.common)?;
    let step = Duration::from_std(args.step).context("step out of range")?;
    let tle = load_tle(&args.common)?;
    let predictor = Predictor::new(&tle)?;
    warn_stale_tle(&predictor, start);
    let mut writer = open_writer(&args.common.out)?;

    if args.common.output_args {
        let start_str = start.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let duration_str = humantime::format_duration(args.common.duration).to_string();
        let step_str = humantime::format_duration(args.step).to_string();
        let frame_str = match args.frame {
            Frame::Teme => "teme",
            Frame::Ecef => "ecef",
        };
        write_args_header(
            &mut *writer,
            &[
                ("command", "state-vectors"),
                ("satellite", &tle.satellite_name),
                ("tle-line1", &tle.line_1),
                ("tle-line2", &tle.line_2),
                ("start", &start_str),
                ("duration", &duration_str),
                ("step", &step_str),
                ("frame", frame_str),
            ],
        )?;
    }

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
