use anyhow::Context as _;
use chrono::Duration;
use sgp4_predict::Predictor;

use super::{
    format_observer_str, load_tle, open_writer, resolve_interval, warn_stale_tle, write_args_header,
};
use crate::{cli::ObservationsArgs, observer, output};

pub fn run(args: ObservationsArgs) -> anyhow::Result<()> {
    let (start, interval) = resolve_interval(&args.common)?;
    let step = Duration::from_std(args.step).context("step out of range")?;
    let tle = load_tle(&args.common)?;
    let observer = match &args.observer.observer {
        Some(s) => observer::parse_observer(s)?,
        None => observer::prompt_observer()?,
    };
    let predictor = Predictor::new(&tle)?;
    warn_stale_tle(&predictor, start);
    let mut writer = open_writer(&args.common.out)?;

    if args.common.output_args {
        let start_str = start.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let duration_str = humantime::format_duration(args.common.duration).to_string();
        let observer_str = format_observer_str(args.observer.observer.as_deref(), &observer);
        let step_str = humantime::format_duration(args.step).to_string();
        write_args_header(
            &mut *writer,
            &[
                ("command", "observations"),
                ("satellite", &tle.satellite_name),
                ("tle-line1", &tle.line_1),
                ("tle-line2", &tle.line_2),
                ("start", &start_str),
                ("duration", &duration_str),
                ("observer", &observer_str),
                ("step", &step_str),
            ],
        )?;
    }

    output::write_observations(
        writer,
        predictor.observation_iter(&observer, interval, step),
    )
}
