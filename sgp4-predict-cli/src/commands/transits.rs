use anyhow::Context as _;
use sgp4_predict::Predictor;

use super::{load_sat, open_writer, resolve_interval, warn_stale_tle};
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
    let writer = open_writer(&args.common.out)?;

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
