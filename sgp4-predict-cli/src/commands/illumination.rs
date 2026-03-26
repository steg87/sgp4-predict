use sgp4_predict::Predictor;

use super::{load_sat, open_writer, resolve_interval, warn_stale_tle};
use crate::{cli::IlluminationArgs, output};

pub fn run(args: IlluminationArgs) -> anyhow::Result<()> {
    let (start, interval) = resolve_interval(&args.common)?;
    let sat = load_sat(&args.common)?;
    let predictor = Predictor::new(&sat)?;
    warn_stale_tle(&predictor, start);
    let writer = open_writer(&args.common.out)?;
    output::write_illumination(writer, predictor.illumination_iter(interval))
}
