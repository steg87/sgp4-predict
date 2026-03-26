use sgp4_predict::Predictor;

use super::{load_sat, open_writer, resolve_interval, warn_stale_tle};
use crate::{cli::ApsidesArgs, output};

pub fn run(args: ApsidesArgs) -> anyhow::Result<()> {
    let (start, interval) = resolve_interval(&args.common)?;
    let sat = load_sat(&args.common)?;
    let predictor = Predictor::new(&sat)?;
    warn_stale_tle(&predictor, start);
    let writer = open_writer(&args.common.out)?;
    output::write_apsides(writer, predictor.apsis_iter(interval))
}
