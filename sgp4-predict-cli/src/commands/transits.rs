use anyhow::Context as _;
use sgp4_predict::Degrees;
use std::path::Path;

use super::{format_observer_str, pairs, prepare};
use crate::{cli::TransitsArgs, output};

pub fn run(args: TransitsArgs, config_path: Option<&Path>) -> anyhow::Result<()> {
    // Resolve the station first: an unknown --gs must fail before the user is
    // asked for a TLE on stdin.
    let observer = args.observer.resolve(config_path)?;
    let opts = args.tuning.build()?;
    let max_elevation_opts = args.tuning.build_max_elevation()?;
    let mut ctx = prepare(&args.common)?;

    let observer_str = format_observer_str(&observer);
    let gs_id = args.observer.gs.as_deref().expect("resolve requires --gs");
    let min_el_str = args.min_elevation_deg.to_string();
    let tuning = args.tuning.header_pairs();
    let refinement = args.refinement.header_pairs();

    let mut extra = vec![
        ("ground-station", gs_id),
        ("observer", observer_str.as_str()),
        ("min-elevation", min_el_str.as_str()),
    ];
    extra.extend(pairs(&[&tuning, &refinement]));
    ctx.write_args_header("transits", &args.common, &extra)?;

    // Set on the predictor rather than passed per call, so the TCA refinement
    // below uses the same root finder as the transit crossings.
    let predictor = ctx
        .predictor
        .clone()
        .with_refinement(args.refinement.build());

    let predictor = &predictor;
    let transits = predictor
        .transits_iter_with_opts(
            &observer,
            ctx.interval.clone(),
            Degrees(args.min_elevation_deg),
            opts,
            args.refinement.build(),
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
                .max_elevation_with_opts(transit, &observer, max_elevation_opts)
                .context("TCA error")?;
            Ok((transit, aos_obs, los_obs, tca_time, tca_obs))
        });

    output::write_transits(&mut ctx.writer, ctx.format, transits)
}
