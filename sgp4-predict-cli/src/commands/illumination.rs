use super::{pairs, prepare};
use crate::{cli::IlluminationArgs, output};

pub fn run(args: IlluminationArgs) -> anyhow::Result<()> {
    let opts = args.tuning.build()?;
    let mut ctx = prepare(&args.common)?;

    let tuning = args.tuning.header_pairs();
    let refinement = args.refinement.header_pairs();
    ctx.write_args_header(
        "illumination",
        &args.common,
        None,
        &pairs(&[&tuning, &refinement]),
    )?;

    let windows = ctx.predictor.illumination_iter_with_opts(
        ctx.interval.clone(),
        opts,
        args.refinement.build(),
    );
    output::write_illumination(ctx.writer, ctx.format, windows)
}
