use anyhow::Context as _;
use sgp4_predict::{Degrees, Predictor};

use super::{
    format_observer_str, load_tle, open_writer, resolve_interval, warn_stale_tle, write_args_header,
};
use crate::{cli::TransitsArgs, output};

pub fn run(args: TransitsArgs) -> anyhow::Result<()> {
    let observer = args.observer.resolve(args.common.config.as_deref())?;
    let (start, interval) = resolve_interval(&args.common)?;
    let tle = load_tle(&args.common)?;
    let predictor = Predictor::from_tle(&tle)?;
    warn_stale_tle(&predictor, start);
    let mut writer = open_writer(&args.common.out)?;

    if args.common.output_args {
        let start_str = start.format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let duration_str = humantime::format_duration(args.common.duration).to_string();
        let observer_str = format_observer_str(&observer);
        let gs_id = args.observer.gs.as_deref().expect("resolve requires --gs");
        let min_el_str = args.min_elevation_deg.to_string();
        write_args_header(
            &mut *writer,
            &[
                ("command", "transits"),
                ("satellite", &tle.satellite_name),
                ("tle-line1", &tle.line_1),
                ("tle-line2", &tle.line_2),
                ("start", &start_str),
                ("duration", &duration_str),
                ("ground-station", gs_id),
                ("observer", &observer_str),
                ("min-elevation", &min_el_str),
            ],
        )?;
    }

    let transits = predictor
        .transits_iter(&observer, interval, Degrees(args.min_elevation_deg))
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
