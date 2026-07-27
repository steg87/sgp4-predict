use super::prepare;
use crate::{cli::IlluminationArgs, output};

pub fn run(args: IlluminationArgs) -> anyhow::Result<()> {
    let mut ctx = prepare(&args.common)?;
    ctx.write_args_header("illumination", &args.common, None, &[])?;

    let windows = ctx.predictor.illumination_iter(ctx.interval.clone());
    output::write_illumination(ctx.writer, ctx.format, windows)
}
