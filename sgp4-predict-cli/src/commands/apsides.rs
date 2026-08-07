use super::{pairs, prepare};
use crate::{cli::ApsidesArgs, output};

pub fn run(args: ApsidesArgs) -> anyhow::Result<()> {
    let mut ctx = prepare(&args.common)?;

    let tuning = args.tuning.header_pairs();
    let refinement = args.refinement.header_pairs();
    ctx.write_args_header(
        "apsides",
        &args.common,
        None,
        &pairs(&[&tuning, &refinement]),
    )?;

    let apsides = ctx.predictor.apsis_iter_with_opts(
        ctx.interval.clone(),
        args.tuning.build()?,
        args.refinement.build(),
    );
    output::write_apsides(ctx.writer, ctx.format, apsides)
}
