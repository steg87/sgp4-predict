use anyhow::Context as _;
use chrono::Duration;
use sgp4_predict::Predictor;

use super::{load_sat, open_writer, resolve_interval, warn_stale_tle};
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
    let writer = open_writer(&args.common.out)?;
    output::write_observations(
        writer,
        predictor.observation_iter(&observer, interval, step),
    )
}
