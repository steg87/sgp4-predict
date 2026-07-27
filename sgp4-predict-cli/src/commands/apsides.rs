use super::prepare;
use crate::{cli::ApsidesArgs, output};

pub fn run(args: ApsidesArgs) -> anyhow::Result<()> {
    let mut ctx = prepare(&args.common)?;
    ctx.write_args_header("apsides", &args.common, None, &[])?;

    let apsides = ctx.predictor.apsis_iter(ctx.interval.clone());
    output::write_apsides(ctx.writer, ctx.format, apsides)
}
