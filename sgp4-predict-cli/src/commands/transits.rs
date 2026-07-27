use anyhow::Context as _;
use sgp4_predict::Degrees;
use std::path::Path;

use super::{effective_config_path, format_observer_str, prepare};
use crate::{cli::TransitsArgs, output};

pub fn run(args: TransitsArgs, config_path: Option<&Path>) -> anyhow::Result<()> {
    // Resolve the station first: an unknown --gs must fail before the user is
    // asked for a TLE on stdin.
    let observer = args.observer.resolve(config_path)?;
    let mut ctx = prepare(&args.common)?;

    let observer_str = format_observer_str(&observer);
    let gs_id = args.observer.gs.as_deref().expect("resolve requires --gs");
    let min_el_str = args.min_elevation_deg.to_string();
    let config = effective_config_path(config_path);
    ctx.write_args_header(
        "transits",
        &args.common,
        config.as_deref(),
        &[
            ("ground-station", gs_id),
            ("observer", &observer_str),
            ("min-elevation", &min_el_str),
        ],
    )?;

    let predictor = &ctx.predictor;
    let transits = predictor
        .transits_iter(
            &observer,
            ctx.interval.clone(),
            Degrees(args.min_elevation_deg),
        )
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

    output::write_transits(&mut ctx.writer, ctx.format, transits)
}
