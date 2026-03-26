use anyhow::Context as _;
use sgp4_predict::Predictor;

use super::{
    format_observer_str, load_sat, open_writer, resolve_interval, warn_stale_tle, write_args_header,
};
use crate::{cli::TransitsArgs, observer, output};

pub fn run(args: TransitsArgs) -> anyhow::Result<()> {
    let (start, interval) = resolve_interval(&args.common)?;
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
        let observer_str = format_observer_str(args.observer.observer.as_deref(), &observer);
        let min_el_str = args.min_elevation.to_string();
        write_args_header(
            &mut *writer,
            &[
                ("command", "transits"),
                ("satellite", &sat.name),
                ("tle-line1", &sat.line1),
                ("tle-line2", &sat.line2),
                ("start", &start_str),
                ("duration", &duration_str),
                ("observer", &observer_str),
                ("min-elevation", &min_el_str),
            ],
        )?;
    }

    let transits = predictor
        .transits_iter(&observer, interval, args.min_elevation)
        .map(|result| {
            let transit = result.context("transit detection error")?;
            let aos_obs = predictor
                .observe_at(transit.start, &observer)
                .context("AoS observation error")?;
            let los_obs = predictor
                .observe_at(transit.end, &observer)
                .context("LoS observation error")?;
            let (tca_time, tca_obs) = predictor
                .max_elevation(transit, &observer)
                .context("TCA error")?;
            Ok((transit, aos_obs, los_obs, tca_time, tca_obs))
        });

    output::write_transits(writer, transits)
}
