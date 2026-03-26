use anyhow::Context as _;
use chrono::Duration;
use sgp4_predict::Predictor;

use super::{load_sat, open_writer, resolve_interval, warn_stale_tle, write_args_header};
use crate::{cli::ObservationsArgs, observer, output};

pub fn run(args: ObservationsArgs) -> anyhow::Result<()> {
    let (start, interval) = resolve_interval(&args.common)?;
    let step = Duration::from_std(args.step).context("step out of range")?;
    let sat = load_sat(&args.common)?;
    let observer = match &args.observer.observer {
        Some(s) => observer::parse_observer(s)?,
        None => observer::prompt_observer()?,
    };
    let predictor = Predictor::new(&sat)?;
    warn_stale_tle(&predictor, start);
    let mut writer = open_writer(&args.common.out)?;

    if args.common.output_args {
        let start_str = start.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let duration_str = humantime::format_duration(args.common.duration).to_string();
        let observer_str = args
            .observer
            .observer
            .as_deref()
            .map(str::to_owned)
            .unwrap_or_else(|| {
                format!(
                    "{},{},{}",
                    observer.lat_deg, observer.lon_deg, observer.alt_m
                )
            });
        let step_str = humantime::format_duration(args.step).to_string();
        write_args_header(
            &mut *writer,
            &[
                ("command", "observations"),
                ("satellite", &sat.name),
                ("tle-line1", &sat.line1),
                ("tle-line2", &sat.line2),
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
